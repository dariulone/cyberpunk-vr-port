#include "Render/NativeDepthPreparation.hpp"
#include <windows.h>
#include <cstdio>
#include <stdexcept>
using namespace cvr::stereo::eager_depth;
static void Require(bool value){if(!value)throw std::runtime_error("Depth preparation invariant failed");}
static uint32_t prepares{},gathers{},filters{},finalizes{};
static bool throwNative{};
static uint64_t rejectSecond{};
static bool ReadMemory(uint64_t source,void* target,size_t bytes){SIZE_T done{};return source &&
    ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<void*>(source),target,bytes,&done) && done==bytes;}
static void* GatherPackets(void* buckets,void* target){
    if(throwNative)throw std::runtime_error("native failure");
    ++gathers;auto& output=*static_cast<Array*>(target);output.count=0;
    auto** heads=static_cast<const OwnedBuckets::Node**>(buckets);
    for(UINT i=0;i<28;++i)for(auto* node=heads[i];node;node=reinterpret_cast<const OwnedBuckets::Node*>(node->next)){
        Require(node->count<=output.capacity-output.count);
        std::copy_n(node->data,node->count,output.data+output.count);output.count+=node->count;
    }
    return target;
}
static int64_t FilterPackets(void* output,char,uint32_t,uint32_t){
    ++filters;auto& array=*static_cast<Array*>(output);
    for(uint32_t i=0;i<array.count;)if(rejectSecond && array.data[i].second==rejectSecond)array.data[i]=array.data[--array.count];else ++i;
    return 0;
}
static void FinalizePackets(void* output,char,int64_t,int64_t){
    ++finalizes;
    auto& array=*static_cast<Array*>(output);
    std::sort(array.data,array.data+array.count,[](const Packet& a,const Packet& b){return a.first<b.first;});
}
static void PreparePackets(void*,void* buckets,void* output,uint32_t flags0,uint32_t flags1){
    Require(!flags0 && !flags1);++prepares;GatherPackets(buckets,output);FilterPackets(output,0,flags0,flags1);FinalizePackets(output,0,0,0);
}
template<class T>static void Put(void* buffer,size_t offset,T value){std::memcpy(static_cast<uint8_t*>(buffer)+offset,&value,sizeof(value));}
struct Fixture {
    std::array<uint8_t,0x2C48> mainView{},eyeView{};
    std::vector<uint8_t> mainStorage=std::vector<uint8_t>(0x3600),eyeStorage=std::vector<uint8_t>(0x3600);
    std::array<uint8_t,32> mainWork{},eyeWork{},mainArgs{},eyeArgs{};
    std::vector<Packet> mainPackets{{3,30},{1,10},{2,20}},eyePackets{{5,50},{4,40},{6,60}};
    OwnedBuckets mainBuckets{mainPackets},eyeBuckets{eyePackets};
    std::array<Packet,16> mainOutput{},eyeOutput{};
    Array mainArray{mainOutput.data(),16,0},eyeArray{eyeOutput.data(),16,0};
    Cache cache;
    Fixture(){
        Put(mainView.data(),0x2C40,reinterpret_cast<uintptr_t>(mainStorage.data()));Put(eyeView.data(),0x2C40,reinterpret_cast<uintptr_t>(eyeStorage.data()));
        Put(mainWork.data(),24,reinterpret_cast<uintptr_t>(mainView.data()));Put(eyeWork.data(),24,reinterpret_cast<uintptr_t>(eyeView.data()));
        Put(mainArgs.data(),0,reinterpret_cast<uintptr_t>(mainWork.data()));Put(eyeArgs.data(),0,reinterpret_cast<uintptr_t>(eyeWork.data()));
        Put(mainArgs.data(),16,uint32_t(3));Put(eyeArgs.data(),16,uint32_t(3));mainArgs[26]=eyeArgs[26]=1;
        std::memcpy(mainStorage.data()+0x34C0,mainBuckets.heads.data(),sizeof(mainBuckets.heads));
        std::memcpy(eyeStorage.data()+0x34C0,eyeBuckets.heads.data(),sizeof(eyeBuckets.heads));
    }
    bool Call(bool eye,uint32_t mode=1,uint32_t frame=1,bool early=true){
        auto* args=eye?eyeArgs.data():mainArgs.data();auto* buckets=(eye?eyeStorage.data():mainStorage.data())+0x34C0;
        auto* output=eye?&eyeArray:&mainArray;
        const bool handled=cache.Intercept({ReadMemory,PreparePackets,GatherPackets,FilterPackets,FinalizePackets},frame,reinterpret_cast<uintptr_t>(mainView.data()),
            reinterpret_cast<uintptr_t>(eyeView.data()),early,mode,args,buckets,output,0,0);
        if(!handled)PreparePackets(args,buckets,output,0,0);return handled;
    }
};
int main()try{
    {Fixture f;Snapshot snapshot;Require(snapshot.Capture(reinterpret_cast<uintptr_t>(f.eyeStorage.data()+0x34C0),ReadMemory));
        Require(snapshot.packets==f.eyePackets && (reinterpret_cast<uintptr_t>(snapshot.packets.data())&15)==0);
        f.eyeBuckets.node.next=reinterpret_cast<uintptr_t>(&f.eyeBuckets.node);
        Require(!snapshot.Capture(reinterpret_cast<uintptr_t>(f.eyeStorage.data()+0x34C0),ReadMemory));
        f.eyeBuckets.node.next=0;f.eyeBuckets.node.count=MaxPackets+1;
        Require(!snapshot.Capture(reinterpret_cast<uintptr_t>(f.eyeStorage.data()+0x34C0),ReadMemory));}
    {Fixture f;prepares=gathers=filters=finalizes=0;Require(f.Call(false) && prepares==1 && finalizes==2 && filters==1);
        Require(f.Call(true) && prepares==1 && finalizes==3 && filters==2);
        Require(f.cache.counts[InputSame]==1 && f.cache.counts[OutputSame]==1 && f.cache.counts[OutputDifferent]==0);
        Require(f.eyeArray.data==f.eyeOutput.data() && f.eyeArray.count==3 && f.eyeOutput[0].first==4);}
    {Fixture f;prepares=gathers=filters=finalizes=0;Require(f.Call(false,2) && prepares==1 && finalizes==2);
        Require(f.Call(true,2) && prepares==1 && finalizes==2 && filters==2);
        Require(f.cache.counts[Replaced]==1 && f.eyeArray.count==3 && f.eyeOutput[0].first==4 && f.eyeArray.data==f.eyeOutput.data());}
    {Fixture f;prepares=0;Require(f.Call(false,2));f.eyePackets[0].first=0;
        Require(f.Call(true,2) && prepares==1 && f.cache.counts[Changed]==1 && f.eyeOutput[0].first==0);}
    {Fixture f;filters=finalizes=0;Require(f.Call(false,2));rejectSecond=50;
        Require(f.Call(true,2) && filters==2 && finalizes==3 && f.eyeArray.count==2 && f.cache.counts[Changed]==1 && f.cache.counts[Replaced]==0);
        Require(std::none_of(f.eyeOutput.begin(),f.eyeOutput.begin()+2,[](const Packet& packet){return packet.second==50;}));rejectSecond=0;}
    {Fixture f;Require(f.Call(false));Require(!f.Call(true,1,2));Require(f.cache.counts[Stale]==1);}
    {Fixture f;Require(!f.Call(true));Require(f.cache.counts[Missing]==1);Require(!f.Call(false,1,1,false));Require(f.cache.counts[TooLate]==1);}
    {Fixture f;throwNative=true;bool propagated=false;
        try{f.Call(false);}catch(const std::runtime_error&){propagated=true;}throwNative=false;Require(propagated);}
    std::puts("PASS owned aligned native bucket snapshots, eager preparation, exact comparison, replacement and invalidation");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
