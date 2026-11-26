#include "Render/NativePreparation.hpp"
#include <cstring>
#include <cstdio>
#include <source_location>
#include <stdexcept>
#include <string>
using namespace cvr::stereo::preparation;
static void Require(bool v,std::source_location at=std::source_location::current()){if(!v)throw std::runtime_error("Preparation trace invariant failed at line "+std::to_string(at.line()));}
template<class T,size_t N>void Put(std::array<uint8_t,N>& bytes,size_t at,T value){std::memcpy(bytes.data()+at,&value,sizeof(value));}
static uint64_t seen[2]{};static uint32_t seenCount{};
static void* Prepare(void* manager,void*,void* requests){auto* bytes=static_cast<uint8_t*>(requests);uint64_t data{};std::memcpy(&data,bytes,8);std::memcpy(&seenCount,bytes+12,4);
    if(seenCount<=2)std::memcpy(seen,reinterpret_cast<const void*>(data),seenCount*8);return manager;}
int main()try {
    std::array<uint8_t,0x2C48> view{};std::array<uint8_t,32> context{},arguments{};
    Put(view,0x2C40,uint64_t(0x9000));Put(context,0x18,reinterpret_cast<uintptr_t>(view.data()));
    Put(arguments,0,reinterpret_cast<uintptr_t>(context.data()));Put(arguments,0x10,uint32_t(3));Put(arguments,0x14,uint32_t(4));arguments[0x18]=1;
    cvr::stereo::packets::Packet packets[]{{1,2},{3,4},{5,6}};
    struct Array{uint64_t data;uint32_t capacity,count;};Array output{reinterpret_cast<uintptr_t>(packets),3,3};
    Require(!Active());CyberpunkVR_PreparationRequest=1;FrameBoundary();Require(Active());
    auto outer=BeginStage(arguments.data(),reinterpret_cast<void*>(0x1000),&output,7,8,1);Require(outer.scope);
    Output(Kind::Gather,&output);packets[0].first=99;output.count=2;Output(Kind::Filter,&output);
    auto inner=BeginStage(arguments.data(),reinterpret_cast<void*>(0x2000),&output,9,10,0);Output(Kind::Gather,&output);EndStage(inner);
    Output(Kind::Finalize,&output);EndStage(outer);
    uint64_t span[]{reinterpret_cast<uintptr_t>(packets),reinterpret_cast<uintptr_t>(packets+2)};Consume(0x33DDA4,1,arguments.data(),span);
    FrameBoundary();Require(!Active() && !CyberpunkVR_PreparationFailure);
    const auto* rows=reinterpret_cast<const Record*>(CyberpunkVR_PreparationRecords);const auto* data=reinterpret_cast<const cvr::stereo::packets::Packet*>(CyberpunkVR_PreparationPackets);
    Require(CyberpunkVR_PreparationCount==9 && rows[1].scope==outer.scope && rows[4].scope==inner.scope && rows[6].scope==outer.scope);
    Require(rows[1].count==3 && data[rows[1].offset].first==1 && data[rows[2].offset].first==99);
    Require(rows[0].stage==3 && rows[0].reserved==4 && rows[0].mode==1 && rows[8].stage==3 && rows[8].count==2);
    Require(rows[8].storage==0x9000 && rows[8].side==1);
    const auto count=CyberpunkVR_PreparationCount.load();Output(Kind::Filter,&output);Require(CyberpunkVR_PreparationCount==count);
    CyberpunkVR_PreparationState=0;CyberpunkVR_PreparationRequest=1;FrameBoundary();
    auto second=BeginStage(arguments.data(),nullptr,&output,0,0,0);output.count=4;Output(Kind::Gather,&output);EndStage(second);
    rows=reinterpret_cast<const Record*>(CyberpunkVR_PreparationRecords);Require(rows[1].status==1);
    output.count=2;auto parent=BeginStage(arguments.data(),nullptr,&output,0,0,0);
    const auto before=CyberpunkVR_PreparationCount.load();auto rejected=BeginStage(nullptr,nullptr,&output,0,0,-1);
    Output(Kind::Gather,&output);EndStage(rejected);Require(CyberpunkVR_PreparationCount==before+1);
    Output(Kind::Gather,&output);EndStage(parent);rows=reinterpret_cast<const Record*>(CyberpunkVR_PreparationRecords);
    Require(rows[before+1].scope==parent.scope && !CyberpunkVR_PreparationFailure);
    std::array<uint8_t,0x2C48> otherView{};Put(view,0x28,uint64_t(7));Put(otherView,0x28,uint64_t(0));Put(otherView,0x2C40,uint64_t(0xA000));
    uint64_t views[]{reinterpret_cast<uintptr_t>(view.data()),reinterpret_cast<uintptr_t>(otherView.data())};std::array<uint8_t,0x58> manager{};
    Put(manager,0x48,reinterpret_cast<uintptr_t>(views));Put(manager,0x54,uint32_t(2));uint64_t requestName=7;Array request{reinterpret_cast<uintptr_t>(&requestName),1,1};
    CyberpunkVR_PreparationPairRequest=1;PreparePair(Prepare,manager.data(),nullptr,&request,7,reinterpret_cast<uintptr_t>(otherView.data()),false);
    Require(CyberpunkVR_PreparationPairRequest==1 && seenCount==1);
    PreparePair(Prepare,manager.data(),nullptr,&request,7,reinterpret_cast<uintptr_t>(otherView.data()),true);
    Require(CyberpunkVR_PreparationPairRequest==0 && CyberpunkVR_PreparationPairState==4 && seenCount==1 && seen[0]==7);
    Require(request.count==1 && requestName==7 && CyberpunkVR_PreparationPairData[7]==0);
    PreparePair(Prepare,manager.data(),nullptr,&request,7,reinterpret_cast<uintptr_t>(otherView.data()),true);Require(seenCount==1);
    std::array<uint8_t,0xFB0> root{};std::array<uint8_t,32> rootArguments{};
    Put(rootArguments,16,reinterpret_cast<uintptr_t>(root.data()));requestName=0;
    Put(root,0x168,uint64_t(1));Put(root,0x170,uint64_t(2));Put(root,0xF98,uint64_t(3));Put(root,0xFA0,uint32_t(4));
    for(auto* v:{&view,&otherView}){Put(*v,0x54,uint32_t(512));Put(*v,0x58,uint32_t(512));}
    CyberpunkVR_PreparationPairRequest=2;PreparePair(Prepare,manager.data(),rootArguments.data(),&request,7,reinterpret_cast<uintptr_t>(otherView.data()),true,reinterpret_cast<uintptr_t>(view.data()));
    Require(CyberpunkVR_PreparationPairState==5 && seenCount==1 && request.count==1);
    Put(root,0xFA0,uint32_t(0));CyberpunkVR_PreparationPairRequest=2;
    PreparePair(Prepare,manager.data(),rootArguments.data(),&request,7,reinterpret_cast<uintptr_t>(otherView.data()),true,reinterpret_cast<uintptr_t>(view.data()));
    Require(CyberpunkVR_PreparationPairState==2 && CyberpunkVR_PreparationPairData[7]==1 && seenCount==2 && seen[0]==0 && seen[1]==7);
    Require(request.count==1 && requestName==0 && CyberpunkVR_PreparationPairRoot[7]==1);
    uint64_t skipName=7;Put(root,0x140,reinterpret_cast<uintptr_t>(&skipName));Put(root,0x148,uint32_t(1));Put(root,0x14C,uint32_t(1));
    CyberpunkVR_PreparationPairRequest=3;CyberpunkVR_PreparationRootSkipped=0;
    Put(root,0xFA0,uint32_t(4));Require(!SuppressRtt(root.data(),7,true));Put(root,0xFA0,uint32_t(0));
    skipName=0;Require(!SuppressRtt(root.data(),7,true));skipName=7;
    Put(root,0x14C,uint32_t(2));Require(!SuppressRtt(root.data(),7,true));Put(root,0x14C,uint32_t(1));
    CyberpunkVR_PreparationPairVerified=0;Require(!SuppressRtt(root.data(),7,true));CyberpunkVR_PreparationPairVerified=1;
    Require(!SuppressRtt(root.data(),7,false) && SuppressRtt(root.data(),7,true));
    Require(!SuppressRtt(root.data(),7,true) && CyberpunkVR_PreparationRootSkipped==1);
    Put(root,0xFA0,uint32_t(0));
    PreparePair(Prepare,manager.data(),rootArguments.data(),&request,7,reinterpret_cast<uintptr_t>(otherView.data()),true,reinterpret_cast<uintptr_t>(view.data()));
    Require(CyberpunkVR_PreparationPairState==2 && CyberpunkVR_PreparationPairRequest==0 && seenCount==2);
    RootCompleted(root.data(),CyberpunkVR_PreparationPairGeneration.load()-1);Require(CyberpunkVR_PreparationPairCompleted==1);
    FrameBoundary();CyberpunkVR_PreparationState=0;CyberpunkVR_PreparationRequest=4;
    CyberpunkVR_PreparationPairRequest=3;CyberpunkVR_PreparationPairMergeLimit=3;FrameBoundary();
    CyberpunkVR_PreparationPairMergeLimit=4;Require(!SuppressRtt(root.data(),7,true));
    CyberpunkVR_PreparationPairMergeLimit=0;Require(!SuppressRtt(root.data(),7,true));CyberpunkVR_PreparationPairMergeLimit=3;
    for(uint32_t merged=1;merged<=3;++merged){
        Require(SuppressRtt(root.data(),7,true));
        PreparePair(Prepare,manager.data(),rootArguments.data(),&request,7,reinterpret_cast<uintptr_t>(otherView.data()),true,reinterpret_cast<uintptr_t>(view.data()));
        Require(CyberpunkVR_PreparationPairMerged==merged && CyberpunkVR_PreparationPairRequest==0);
        Require(!SuppressRtt(root.data(),7,true));RootCompleted(root.data(),CyberpunkVR_PreparationPairGeneration.load()-1);
        Require(CyberpunkVR_PreparationPairCompleted==merged && CyberpunkVR_PreparationPairRequest==(merged<3?3:0));
        Require(!SuppressRtt(root.data(),7,true));FrameBoundary();
    }
    Put(root,0x170,uint64_t(0));RootCompleted(root.data(),CyberpunkVR_PreparationPairGeneration.load());
    Require(CyberpunkVR_PreparationPairState==2 && CyberpunkVR_PreparationPairVerified==1);
    RootCompleted(root.data(),CyberpunkVR_PreparationPairGeneration.load()-1);
    Require(CyberpunkVR_PreparationPairState==6 && CyberpunkVR_PreparationPairVerified==0 && CyberpunkVR_PreparationPairRequest==0);
    CyberpunkVR_PreparationPairMergeLimit=1;
    FrameBoundary();CyberpunkVR_PreparationState=0;CyberpunkVR_PreparationRequest=1;FrameBoundary();
    auto* list=reinterpret_cast<ID3D12CommandList*>(0x1234);ResetList(list);
    {DrawScope scope(0x33DDA4,1,arguments.data());DrawRecorded(list);DrawRecorded(list);
        {DrawScope nested(0x9999,0,arguments.data());DrawRecorded(list);}
        DrawRecorded(list);}
    auto previousCopy=SetCopySource({reinterpret_cast<uintptr_t>(view.data()),0x1F8928,1});
    EyeCopyRecorded(list,reinterpret_cast<void*>(0x2000),reinterpret_cast<void*>(0x3000));SetCopySource(previousCopy);
    SubmitLists(reinterpret_cast<void*>(0x4000),1,&list);FrameBoundary();
    rows=reinterpret_cast<const Record*>(CyberpunkVR_PreparationRecords);
    Require(CyberpunkVR_PreparationCount==5 && rows[0].kind==16 && rows[1].kind==13 && rows[2].kind==14 && rows[2].flags0==3);
    Require(rows[3].kind==12 && rows[3].view==reinterpret_cast<uintptr_t>(view.data()) && rows[3].flags0==0x1F8928 && rows[3].side==1);
    Require(rows[4].kind==15 && rows[4].context==0x1234 && rows[4].input==0x4000 && rows[4].flags1==1);
    Put(root,0x170,uint64_t(2));CyberpunkVR_PreparationState=0;CyberpunkVR_PreparationRequest=1;FrameBoundary();
    CyberpunkVR_PreparationPairRequest=2;
    PreparePair(Prepare,manager.data(),rootArguments.data(),&request,7,reinterpret_cast<uintptr_t>(otherView.data()),true,reinterpret_cast<uintptr_t>(view.data()));
    FrameBoundary();CyberpunkVR_PreparationPairCompatible=1;CyberpunkVR_PreparationRootCompatible=1;
    std::array<uint8_t,32> mainContext=context,mainArguments=arguments;
    Put(mainContext,0x18,reinterpret_cast<uintptr_t>(otherView.data()));Put(mainArguments,0,reinterpret_cast<uintptr_t>(mainContext.data()));
    auto renderPair=[&]{
        ResetList(list);Require(SuppressRtt(root.data(),7,true));
        PreparePair(Prepare,manager.data(),rootArguments.data(),&request,7,reinterpret_cast<uintptr_t>(otherView.data()),true,reinterpret_cast<uintptr_t>(view.data()));
        RootCompleted(root.data(),CyberpunkVR_PreparationPairGeneration.load()-1);Require(RunActive());
        Consume(0x33DDA4,0,mainArguments.data(),span);Consume(0x33DDA4,1,arguments.data(),span);
        previousCopy=SetCopySource({reinterpret_cast<uintptr_t>(view.data()),0x1EE760,1});
        EyeCopyRecorded(list,reinterpret_cast<void*>(0x2000),reinterpret_cast<void*>(0x3000));SetCopySource(previousCopy);
        SubmitLists(reinterpret_cast<void*>(0x4000),1,&list);
    };
    const auto traceCount=CyberpunkVR_PreparationCount.load();
    CyberpunkVR_PreparationRunRequest=30;FrameBoundary();Require(RunActive() && !Active());
    for(uint32_t i=0;i<30;++i){renderPair();FrameBoundary();}
    Require(CyberpunkVR_PreparationRunState==2 && !CyberpunkVR_PreparationRunFailure && CyberpunkVR_PreparationRunFrames==30);
    Require(CyberpunkVR_PreparationPairCompleted==30 && CyberpunkVR_PreparationRunDepth[0]==30 && CyberpunkVR_PreparationRunDepth[1]==30);
    Require(CyberpunkVR_PreparationRunEyeCopies==30 && CyberpunkVR_PreparationRunEyeSubmissions==30 && CyberpunkVR_PreparationCount==traceCount);
    CyberpunkVR_PreparationRunRequest=60;FrameBoundary();renderPair();CyberpunkVR_PreparationRunStop=1;
    Put(root,0x170,uint64_t(0));RootCompleted(root.data(),CyberpunkVR_PreparationPairGeneration.load());
    Require(!CyberpunkVR_PreparationRunFailure);Put(root,0x170,uint64_t(2));FrameBoundary();
    Require(CyberpunkVR_PreparationRunState==2 && CyberpunkVR_PreparationPairCompleted==1 && !CyberpunkVR_PreparationPairRequest);
    CyberpunkVR_PreparationRunRequest=60;FrameBoundary();ResetList(list);Require(SuppressRtt(root.data(),7,true));
    const auto admittedGeneration=CyberpunkVR_PreparationPairGeneration.load();CyberpunkVR_PreparationRunStop=1;
    Require(!SuppressRtt(root.data(),7,true));
    PreparePair(Prepare,manager.data(),rootArguments.data(),&request,7,reinterpret_cast<uintptr_t>(otherView.data()),true,reinterpret_cast<uintptr_t>(view.data()));
    RootCompleted(root.data(),admittedGeneration);
    Require(CyberpunkVR_PreparationPairMerged==1 && CyberpunkVR_PreparationPairCompleted==1 && !CyberpunkVR_PreparationPairRequest);
    FrameBoundary();Require(CyberpunkVR_PreparationRunState==2 && !CyberpunkVR_PreparationRunFailure);
    CyberpunkVR_PreparationRunRequest=1201;FrameBoundary();Require(CyberpunkVR_PreparationRunState==3 && CyberpunkVR_PreparationRunFailure==1);
    CyberpunkVR_PreparationRunRequest=30;FrameBoundary();
    EyeCopyRecorded(list,reinterpret_cast<void*>(0x2000),reinterpret_cast<void*>(0x3000));FrameBoundary();
    Require(CyberpunkVR_PreparationRunState==3 && CyberpunkVR_PreparationRunFailure==6 && !CyberpunkVR_PreparationPairRequest);
    CyberpunkVR_PreparationRunState=0;CyberpunkVR_PreparationRunStop=0;
    CyberpunkVR_PreparationState=0;CyberpunkVR_PreparationRequest=1;FrameBoundary();
    std::array<uint8_t,0x168> task{};Put(task,24,reinterpret_cast<uintptr_t>(view.data()));Put(task,80,uint64_t(0xABCD));Put(task,352,uint32_t(3));
    auto materialized=BeginMaterialize(task.data(),1);task.fill(0);EndMaterialize(materialized);FrameBoundary();
    rows=reinterpret_cast<const Record*>(CyberpunkVR_PreparationRecords);
    Require(CyberpunkVR_PreparationCount==2 && rows[0].kind==18 && rows[1].kind==19 && rows[1].view==reinterpret_cast<uintptr_t>(view.data()) &&
        rows[1].output==0xABCD && rows[1].flags0==3 && rows[1].side==1);
    CyberpunkVR_PreparationState=0;CyberpunkVR_PreparationRequest=1;FrameBoundary();
    std::array<uint8_t,0x50> proxyContext{};Put(proxyContext,0x18,reinterpret_cast<uintptr_t>(view.data()));Put(proxyContext,0x48,uint64_t(0x9000));
    std::array<uint64_t,32> proxyTable{};proxyTable[31]=0x12345678;proxyTable[30]=0x87654321;
    std::array<uint64_t,3> proxy{reinterpret_cast<uintptr_t>(proxyTable.data()),0,0};
    uint64_t ids[]{reinterpret_cast<uintptr_t>(proxy.data())+16,reinterpret_cast<uintptr_t>(proxy.data())+16};
    uint64_t idRange[]{reinterpret_cast<uintptr_t>(ids),reinterpret_cast<uintptr_t>(ids+2)};
    auto gathered=BeginProxyGather(proxyContext.data(),idRange,0);ids[0]=0;proxyTable[31]=0;
    EndProxyGather(gathered);rows=reinterpret_cast<const Record*>(CyberpunkVR_PreparationRecords);
    data=reinterpret_cast<const cvr::stereo::packets::Packet*>(CyberpunkVR_PreparationPackets);
    Require(CyberpunkVR_PreparationCount==2 && rows[0].kind==20 && rows[1].kind==21 && rows[0].count==4 && rows[0].flags0==2 && !rows[0].status &&
        rows[1].offset==rows[0].offset && data[rows[0].offset].first==ids[1] && data[rows[0].offset].second==0x12345678 &&
        data[rows[0].offset+1].first==reinterpret_cast<uintptr_t>(proxyTable.data()) && data[rows[0].offset+1].second==0x87654321);
    EndProxyGather(BeginProxyGather(proxyContext.data(),idRange,1));
    rows=reinterpret_cast<const Record*>(CyberpunkVR_PreparationRecords);Require(rows[2].status==1 && !rows[2].count);
    idRange[1]=idRange[0]+1;EndProxyGather(BeginProxyGather(proxyContext.data(),idRange,1));
    rows=reinterpret_cast<const Record*>(CyberpunkVR_PreparationRecords);Require(rows[4].status==1);
    idRange[1]=idRange[0]+32769ull*8;EndProxyGather(BeginProxyGather(proxyContext.data(),idRange,1));
    rows=reinterpret_cast<const Record*>(CyberpunkVR_PreparationRecords);Require(rows[6].status==2);
    Require(!BeginProxyGather(proxyContext.data(),idRange,-1).epoch);
    FrameBoundary();Require(!BeginProxyGather(proxyContext.data(),idRange,0).epoch);
    CyberpunkVR_PreparationState=0;CyberpunkVR_PreparationCameraTrace=1;CyberpunkVR_PreparationRequest=8;
    CyberpunkVR_PreparationPairRequest=3;CyberpunkVR_PreparationPairMergeLimit=8;FrameBoundary();
    Require(!SuppressRtt(root.data(),7,true));CyberpunkVR_PreparationPairMergeLimit=7;
    for(uint32_t i=1;i<=7;++i){
        Require(Active() && SuppressRtt(root.data(),7,true));
        PreparePair(Prepare,manager.data(),rootArguments.data(),&request,7,reinterpret_cast<uintptr_t>(otherView.data()),true,reinterpret_cast<uintptr_t>(view.data()));
        RootCompleted(root.data(),CyberpunkVR_PreparationPairGeneration.load()-1);Require(CyberpunkVR_PreparationPairCompleted==i);FrameBoundary();
    }
    Require(Active() && !CyberpunkVR_PreparationPairRequest);FrameBoundary();Require(!Active());
    CyberpunkVR_PreparationCameraTrace=0;CyberpunkVR_PreparationPairMergeLimit=1;
    std::puts("PASS owned preparation, bounded paired roots, scoped native draws and output submission evidence");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
