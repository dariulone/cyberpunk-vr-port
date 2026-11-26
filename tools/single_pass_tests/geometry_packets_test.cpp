#include "Render/NativeGeometryPackets.hpp"
#include "Render/NativePacketInputs.hpp"
#include <windows.h>
#include <array>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <thread>
#include <vector>
using namespace cvr::stereo::packets;
void Require(bool value,const char* text){if(!value){std::cerr<<text<<'\n';std::exit(1);}}
void Start(unsigned frames=1){CyberpunkVR_GeometryPacketState=0;CyberpunkVR_GeometryPacketRequest=frames;FrameBoundary();}
struct Arguments {
    std::array<uint8_t,32> bytes{};
    std::array<uint8_t,32> context{};
    std::array<uint8_t,0x2c48> view{};
    Arguments(){const auto c=reinterpret_cast<uintptr_t>(context.data()),v=reinterpret_cast<uintptr_t>(view.data());
        const uint64_t storage=0xabc123;std::memcpy(bytes.data(),&c,8);std::memcpy(context.data()+0x18,&v,8);std::memcpy(view.data()+0x2c40,&storage,8);}
};
Ticket Capture(Arguments& args,const void* begin,const void* end,int side=1,uint32_t node=0x33DDA4){
    const void* range[]{begin,end};return Begin(node,side,reinterpret_cast<void*>(0x123456),args.bytes.data(),range);
}
int main(){
    Arguments args;std::array<Packet,3> data{{{1,2},{1,2},{3,4}}};
    Require(Capture(args,data.data(),data.data()+3).index==UINT32_MAX,"disabled");
    Start();Require(Capture(args,data.data(),data.data()+3,2).index==UINT32_MAX,"unrelated view");
    Require(Capture(args,data.data(),data.data()+3,1,0).index==UINT32_MAX,"unrelated node");
    auto ticket=Capture(args,data.data(),data.data()+3);End(ticket);
    auto row=CyberpunkVR_GeometryPacketRecords[0];
    Require(row.status==0 && row.count==3 && row.storage==0xabc123 && row.endQpc>=row.beginQpc,"valid metadata/timing");
    data[0]={9,10};Require(CyberpunkVR_GeometryPacketData[0].first==1 && CyberpunkVR_GeometryPacketData[1].first==1,"owned data and duplicates");
    FrameBoundary();Require(CyberpunkVR_GeometryPacketState==2,"frame bound");
    Require(Capture(args,data.data(),data.data()+3).index==UINT32_MAX,"completed recorder");
    Start();auto fresh=Capture(args,data.data(),data.data()+3);End(ticket);
    Require(CyberpunkVR_GeometryPacketRecords[fresh.index].endQpc==0,"late completion must not modify new capture");End(fresh);
    Start();Capture(args,nullptr,nullptr);Capture(args,data.data()+2,data.data());
    Capture(args,reinterpret_cast<void*>(0x10000),reinterpret_cast<void*>(0x10001));
    Capture(args,reinterpret_cast<void*>(0x10000),reinterpret_cast<void*>(0x10000+16ull*(MaxSpanPackets+1)));
    auto* guard=VirtualAlloc(nullptr,4096,MEM_RESERVE|MEM_COMMIT,PAGE_NOACCESS);Require(guard!=nullptr,"guard page");
    Capture(args,guard,static_cast<uint8_t*>(guard)+16);
    const void* span[]{data.data(),data.data()+3};Begin(0x33DDA4,1,nullptr,guard,span);
    VirtualFree(guard,0,MEM_RELEASE);
    const Status expected[]{Status::Empty,Status::BadSpan,Status::BadSpan,Status::TooLarge,Status::Unreadable,Status::BadArguments};
    for(unsigned i=0;i<6;++i)Require(CyberpunkVR_GeometryPacketRecords[i].status==static_cast<unsigned>(expected[i]),"malformed span/arguments rejected");
    Require(CyberpunkVR_GeometryPacketDataCount==0,"invalid inputs publish no packets");
    Start();std::vector<std::thread> threads;
    for(unsigned n=0;n<8;++n)threads.emplace_back([&]{for(unsigned i=0;i<20;++i)End(Capture(args,data.data(),data.data()+3));});
    for(auto& thread:threads)thread.join();Require(CyberpunkVR_GeometryPacketCount==160 && CyberpunkVR_GeometryPacketDataCount==480,"concurrent capture");
    for(unsigned i=0;i<160;++i){const auto& r=CyberpunkVR_GeometryPacketRecords[i];Require(r.sequence==i+1 && r.offset==i*3 && r.count==3 && r.endQpc>=r.beginQpc,"record publication");}
    for(unsigned i=0;i<RecordCapacity;++i)End(Capture(args,nullptr,nullptr));Require(CyberpunkVR_GeometryPacketState==3 && CyberpunkVR_GeometryPacketCount==RecordCapacity,"record capacity");
    Start();std::vector<Packet> large(MaxSpanPackets,{11,12});
    for(unsigned i=0;i<PacketCapacity/MaxSpanPackets;++i)End(Capture(args,large.data(),large.data()+large.size()));
    End(Capture(args,data.data(),data.data()+3));Require(CyberpunkVR_GeometryPacketState==3 && CyberpunkVR_GeometryPacketDataCount==PacketCapacity,"packet capacity");
    Start(100);for(unsigned i=0;i<3;++i){FrameBoundary();Require(CyberpunkVR_GeometryPacketState==1,"four frame request clamp");}FrameBoundary();Require(CyberpunkVR_GeometryPacketState==2,"clamped end");
    Require((CyberpunkVR_GeometryPacketSeq.load()&1)==0,"publication version even");
    CyberpunkVR_GeometryPacketResolve=1;Start();End(Capture(args,data.data(),data.data()+3));
    Require(CyberpunkVR_GeometryPacketInputFailure==3 && CyberpunkVR_GeometryPacketInputCount==3 && CyberpunkVR_GeometryPacketInputs!=0,"missing native pool fails without dereference");
    FrameBoundary();CyberpunkVR_GeometryPacketResolve=0;FrameBoundary();
    Require(CyberpunkVR_GeometryPacketInputCount==0 && CyberpunkVR_GeometryPacketInputs==0,"optional input storage released after disabling");
    std::cout<<"Native packet capture: ownership, duplicates, malformed memory, concurrency and bounds passed\n";
}
