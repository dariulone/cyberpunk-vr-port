#include "Framegen/DisplayProbe.hpp"
#include <dxgi1_6.h>
#include <d3d12sdklayers.h>
#include <iostream>
#include <vector>
#include <cstdlib>
using namespace cvr::framegen;
using Microsoft::WRL::ComPtr;
void check(bool v,const char* why){if(!v){std::cerr<<why<<'\n';std::exit(1);}}
void hr(HRESULT h){check(SUCCEEDED(h),"D3D12 failure");}
void await(ID3D12CommandQueue* q,ID3D12Fence* f,uint64_t v){
    hr(q->Signal(f,v));HANDLE e=CreateEventW(nullptr,FALSE,FALSE,nullptr);check(e!=nullptr,"event");
    hr(f->SetEventOnCompletion(v,e));check(WaitForSingleObject(e,5000)==WAIT_OBJECT_0,"test GPU completion");CloseHandle(e);
}
void seal(DisplayDeadline& d,double now){
    d.probeMs=now;d.probeWidthMs=.01;d.remainingMs=8;d.marginMs=3;d.periodMs=11.111111;
    d.handoffMs=now+.02;d.returnedMs=now+.03;d.kind=1;d.accepted=d.sealed=true;
}
int main(){
    // Known CPU/QPC timelines exercise early GPU with late handoff, late GPU
    // despite timely handoff, future runtime boundary, rejection and uncertainty.
    DisplayDeadline d;seal(d,100);
    auto v=AttributeDisplay(d,102);check(v && v->gpuFromBoundaryMs==-6 && v->handoffFromTargetMs<0,"timely CPU/GPU");
    v=AttributeDisplay(d,112);check(v && v->gpuFromBoundaryMs==4 && v->handoffFromTargetMs<0,"GPU delay distinct from CPU handoff");
    d.handoffMs=110;d.returnedMs=111;v=AttributeDisplay(d,102);
    check(v && v->gpuFromHandoffMs==-8 && v->handoffFromTargetMs==5,"CPU handoff delay distinct from GPU work");
    d.probeWidthMs=.5;check(!AttributeDisplay(d,102),"wide CPU probe rejected");
    seal(d,100);d.remainingMs=15;v=AttributeDisplay(d,102);check(v && v->futureBoundary,"runtime rollover is explicit");
    d.accepted=false;check(!AttributeDisplay(d,102),"rejected XR call cannot look delivered");
    seal(d,100);d.remainingMs=0;check(!AttributeDisplay(d,102),"invalid remaining rejected");

    ComPtr<ID3D12Debug> debug;if(SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug))))debug->EnableDebugLayer();
    ComPtr<IDXGIFactory4> factory;hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));
    ComPtr<ID3D12Device> device;
    for(UINT i=0;!device;++i){ComPtr<IDXGIAdapter1> adapter;if(factory->EnumAdapters1(i,&adapter)==DXGI_ERROR_NOT_FOUND)break;
        DXGI_ADAPTER_DESC1 desc{};adapter->GetDesc1(&desc);if(!(desc.Flags&DXGI_ADAPTER_FLAG_SOFTWARE))D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&device));}
    check(device!=nullptr,"hardware GPU required for calibrated queue probe");
    ComPtr<ID3D12CommandQueue> queue;D3D12_COMMAND_QUEUE_DESC q{};hr(device->CreateCommandQueue(&q,IID_PPV_ARGS(&queue)));
    ComPtr<ID3D12Fence> block,done;hr(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&block)));hr(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&done)));
    DisplayProbePool pool;
    hr(queue->Wait(block.Get(),1));
    for(unsigned i=0;i<4;++i){auto* sample=pool.Begin(queue.Get());check(sample!=nullptr,"four probe slots");seal(*sample,DeadlineQpcMs());}
    check(!pool.Begin(queue.Get()),"full pool skips without overwriting pending queries");
    unsigned collected=0;pool.Poll([&](const auto&,const auto&){++collected;});check(collected==0,"no reading unfinished GPU data");
    Sleep(25);hr(block->Signal(1));await(queue.Get(),done.Get(),1);
    pool.Poll([&](const auto& sample,const auto& result){
        check(result.has_value(),"valid calibrated GPU timestamp");auto a=AttributeDisplay(sample,result->endQpcMs);
        check(a && a->gpuFromBoundaryMs>10 && a->handoffFromTargetMs<0 && result->queueMs>20,
            "real GPU gate delay correctly identified despite timely CPU handoff");++collected;
    });check(collected==4,"all completed samples collected exactly once");
    pool.Poll([&](const auto&,const auto&){++collected;});check(collected==4,"no duplicate samples");
    // Reuse allocations after GPU completion; retain unsealed metadata until
    // xrEndFrame returns. CPU late while GPU ready must not be blamed on GPU.
    for(unsigned i=0;i<16;++i){
        auto* sample=pool.Begin(queue.Get());check(sample!=nullptr,"completed slot reusable");await(queue.Get(),done.Get(),i+2);
        pool.Poll([&](const auto&,const auto&){check(false,"unsealed handoff read prematurely");});
        const auto start=DeadlineQpcMs();seal(*sample,start);Sleep(12);
        sample->handoffMs=DeadlineQpcMs();sample->returnedMs=sample->handoffMs;
        pool.Poll([&](const auto& observed,const auto& result){
            check(result.has_value(),"reuse timestamp valid");auto a=AttributeDisplay(observed,result->endQpcMs);
            check(a && a->gpuFromBoundaryMs<0 && a->handoffFromTargetMs>5 && a->gpuFromHandoffMs< -10,
                "ready GPU followed by deliberately late CPU handoff identified");++collected;
        });
    }
    check(collected==20,"bounded pool reused for all later samples");
    ComPtr<ID3D12InfoQueue> info;if(SUCCEEDED(device.As(&info)))for(UINT64 i=0;i<info->GetNumStoredMessages();++i){
        SIZE_T size{};info->GetMessage(i,nullptr,&size);std::vector<char> bytes(size);auto* msg=reinterpret_cast<D3D12_MESSAGE*>(bytes.data());info->GetMessage(i,msg,&size);
        if(msg->Severity<=D3D12_MESSAGE_SEVERITY_ERROR){std::cerr<<msg->pDescription<<'\n';check(false,"D3D12 validation");}
    }
    std::cout<<"PASS: calibrated late GPU vs late handoff, bounded pool exhaustion/reuse, unfinished/unsealed guards and deadline validation\n";
}
