#include "Render/NativeMaterialBinding.hpp"
#include <cstdio>
#include <map>
#include <stdexcept>
#include <vector>

using namespace cvr::stereo::material_binding;
namespace {
struct Memory {
    std::map<uint64_t,std::vector<uint8_t>> blocks;
    uint64_t calls{},mutateAt{},mutationAddress{};
    bool failed{};
};
void Require(bool good){if(!good)throw std::runtime_error("material binding assertion failed");}
template<class T> void Put(Memory& memory,uint64_t address,const T& value){
    auto& bytes=memory.blocks[address];bytes.resize(sizeof(value));std::memcpy(bytes.data(),&value,sizeof(value));
}
bool Read(void* user,uint64_t address,void* out,size_t bytes){
    auto& memory=*static_cast<Memory*>(user);
    if(memory.failed)return false;
    if(++memory.calls==memory.mutateAt)Put(memory,memory.mutationAddress,uint64_t(0xdead00));
    auto it=memory.blocks.upper_bound(address);if(it==memory.blocks.begin())return false;--it;
    const auto offset=address-it->first;
    if(offset>it->second.size() || bytes>it->second.size()-offset)return false;
    std::memcpy(out,it->second.data()+offset,bytes);return true;
}
constexpr uint64_t game=0x1000000,registry=0x5000000,layout=0x30000,table=0x40000;
constexpr uint32_t handle=2,parameter=19;
constexpr auto recordAddress=registry+0x5C0AF8+(handle-1)*0xB0;
constexpr auto entry=layout+0x38+parameter*16;
constexpr uint64_t descriptor=0x70000,address=0x900100;
const cvr::stereo::camera_upload::Context context{0x10000,0x20000,0x60000};
void PutRecord(Memory& memory,uint8_t type=0x36,uint32_t size=292,uint64_t gpu=address,uint64_t resource=0x80000){
    std::array<uint8_t,0x50> bytes{};std::memcpy(bytes.data(),&size,4);bytes[6]=type;
    std::memcpy(bytes.data()+8,&gpu,8);std::memcpy(bytes.data()+16,&resource,8);
    std::memcpy(bytes.data()+0x48,&descriptor,8);Put(memory,recordAddress,bytes);
}
Memory Fixture(){
    Memory memory;
    Put(memory,game+0x3BCDCA8,registry);Put(memory,context.context+0x30,context.list);
    Put(memory,context.context+0x60,context.state);Put(memory,context.state+0x68,layout);
    Put(memory,context.state+0x28,table);Put(memory,layout+0x477,uint8_t(parameter));
    Put(memory,entry,uint32_t(2));Put(memory,entry+4,uint32_t(50));
    Put(memory,entry+8,uint16_t(4));Put(memory,entry+10,uint16_t(1));
    Put(memory,table+50*8,descriptor);PutRecord(memory);return memory;
}
bool SnapshotOf(Memory& memory,Snapshot& out){return detail::ReadSnapshot(Read,&memory,game,context,handle,out);}
}
int main()try{
    auto memory=Fixture();Snapshot registered;
    Require(SnapshotOf(memory,registered) && registered.slot.parameter==19 && registered.slot.index==50 &&
        registered.slot.tableOffset==0 && registered.record.bytes==292 && registered.record.kind==3);
    const auto native=detail::Native(registered);
    Require(native.resource==0x80000 && native.gpu==address && native.bytes==292 && native.context==context.context);
    Require(detail::FitsCbv(registered,address,512));
    Require(!detail::FitsCbv(registered,address,256) && !detail::FitsCbv(registered,address+256,512));
    Require(!detail::FitsCbv(registered,address+1,512) && !detail::FitsCbv(registered,address,513));
    Require(!detail::FitsCbv(registered,address,65792));
    Require(detail::MatchesDraw(registered,registered,19,0,address,512,address,512));
    Require(!detail::MatchesDraw(registered,registered,18,0,address,512,address,512));
    Require(!detail::MatchesDraw(registered,registered,19,1,address,512,address,512));
    Require(!detail::MatchesDraw(registered,registered,19,0,address,512,address+256,512));
    auto changed=registered;changed.record.resource+=8;
    Require(!detail::MatchesDraw(registered,changed,19,0,address,512,address,512));
    changed=registered;changed.context.list+=8;
    Require(!detail::MatchesDraw(registered,changed,19,0,address,512,address,512));
    memory=Fixture();PutRecord(memory,0x46,292,UINT64_MAX,UINT64_MAX);Snapshot transient;
    Require(SnapshotOf(memory,transient) && transient.record.kind==4 && !transient.record.resource &&
        !transient.record.gpu && !transient.record.descriptor && transient.slot.descriptor==descriptor);
    Require(!detail::Native(transient).resource && detail::FitsCbv(transient,address,512));
    for(const auto type:{uint8_t(0x30),uint8_t(0x35),uint8_t(0x3d)}){
        memory=Fixture();PutRecord(memory,type);Require(!SnapshotOf(memory,changed));
    }
    for(const auto size:{0u,291u}){memory=Fixture();PutRecord(memory,0x36,size);Require(!SnapshotOf(memory,changed));}
    memory=Fixture();PutRecord(memory,0x36,292,address,0);Require(!SnapshotOf(memory,changed));
    memory=Fixture();PutRecord(memory,0x36,512,UINT64_MAX-255);Require(!SnapshotOf(memory,changed));
    memory=Fixture();Put(memory,table+50*8,descriptor+8);Require(!SnapshotOf(memory,changed));
    memory=Fixture();Put(memory,context.context+0x30,context.list+8);Require(!SnapshotOf(memory,changed));
    memory=Fixture();Put(memory,context.context+0x60,context.state+8);Require(!SnapshotOf(memory,changed));
    for(const auto p:{uint8_t(64),uint8_t(255)}){
        memory=Fixture();Put(memory,layout+0x477,p);Require(!SnapshotOf(memory,changed));
    }
    memory=Fixture();Put(memory,entry,uint32_t(0));Require(!SnapshotOf(memory,changed));
    memory=Fixture();Put(memory,entry+8,uint16_t(5));Require(!SnapshotOf(memory,changed));
    memory=Fixture();Put(memory,entry+10,uint16_t(0));Require(!SnapshotOf(memory,changed));
    memory=Fixture();Put(memory,entry+8,uint16_t(3));Require(!SnapshotOf(memory,changed));
    Put(memory,entry+10,uint16_t(2));Put(memory,table+51*8,descriptor);
    Require(SnapshotOf(memory,changed) && changed.slot.tableOffset==1 && changed.slot.index==51);
    memory=Fixture();Put(memory,entry+4,uint32_t(8192));Require(!SnapshotOf(memory,changed));
    memory=Fixture();Put(memory,context.state+0x28,UINT64_MAX-8);Require(!SnapshotOf(memory,changed));
    memory=Fixture();memory.failed=true;Require(!SnapshotOf(memory,changed));
    memory=Fixture();Snapshot once;
    Require(detail::ReadSnapshotOnce(Read,&memory,game,context,handle,once));const auto reads=memory.calls;
    memory=Fixture();memory.mutateAt=reads+1;memory.mutationAddress=table+50*8;
    Require(!SnapshotOf(memory,changed));
    memory=Fixture();memory.mutateAt=reads+1;memory.mutationAddress=game+0x3BCDCA8;
    Require(!SnapshotOf(memory,changed));
    memory=Fixture();
    for(const auto invalid:{0u,0x100001u,UINT32_MAX})Require(!detail::ReadSnapshot(Read,&memory,game,context,invalid,changed));
    Require(!detail::ReadSnapshot(Read,&memory,UINT64_MAX,context,handle,changed));
    Require(!detail::ReadSnapshot(Read,&memory,game,{},handle,changed));
    Require(!detail::ReadSnapshot(nullptr,&memory,game,context,handle,changed));
    std::puts("PASS material b4 native identity, transient separation, logical CBV bounds, torn reads and draw binding checks");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
