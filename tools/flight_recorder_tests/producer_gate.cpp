#include "Framegen/ProducerGate.hpp"
#include <dxgi1_6.h>
#include <d3d12sdklayers.h>
#include <thread>
#include <barrier>
#include <chrono>
#include <iostream>
#include <vector>
#include <cstdlib>
using namespace cvr::framegen;
using Microsoft::WRL::ComPtr;
void check(bool ok,const char* text){if(!ok){std::cerr<<text<<'\n';std::exit(1);}}
void hr(HRESULT h){check(SUCCEEDED(h),"D3D12 operation failed");}
int main(){
    ComPtr<ID3D12Debug> debug;if(SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug))))debug->EnableDebugLayer();
    ComPtr<IDXGIFactory4> factory;hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));
    ComPtr<IDXGIAdapter> adapter;hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)));
    ComPtr<ID3D12Device> device;hr(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&device)));
    auto fence=[&]{ComPtr<ID3D12Fence> f;hr(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&f)));return f;};
    using R=ProducerGateResult;
    double time=0;unsigned waits=0;auto now=[&]{return time;};
    auto noWait=[&](double){++waits;return true;};
    check(GateProducer({},5,now,noWait).result==R::Missing && !waits,"missing receipt cannot wait");
    auto f=fence();ProducerReceipt receipt{f,1};
    check(GateProducer(receipt,0,now,noWait).result==R::Missing,"disabled budget bypass");
    auto result=GateProducer(receipt,5,now,[&](double ms){time+=ms;if(time>=1)hr(f->Signal(1));return true;});
    check(result.result==R::Released && result.elapsedMs==1 && result.waits==2,"release on exact completion");
    check(GateProducer(receipt,5,now,noWait).result==R::Ready && !waits,"ready frame adds no wait");
    f=fence();receipt={f,1};time=0;
    result=GateProducer(receipt,5.5,now,[&](double ms){check(ms<=.5,"wait slice cap");time+=ms;return true;});
    check(result.result==R::Timeout && result.elapsedMs==5.5,"pending GPU has bounded timeout");
    result=GateProducer(receipt,5,now,[](double){return false;});
    check(result.result==R::WaitFailed && result.waits==1,"timer failure returns immediately");
    result=GateProducer(receipt,100,now,[](double){return true;});
    check(result.result==R::Timeout && result.waits==16,"stalled clock cannot loop forever");
    check(std::abs(ProducerGateBudgetMs(11111111)-5.5555555)<.0001,"90Hz bound");
    check(ProducerGateBudgetMs(8333333)<4.167 && ProducerGateBudgetMs(13888889)<6.945,"120/72Hz bounds");
    check(ProducerGateBudgetMs(0)==0 && ProducerGateBudgetMs(40000000)==8,"invalid/slow refresh bounds");

    // Receipts cannot leak between migrated Present jobs or failed captures.
    std::barrier sync(2);
    auto worker=[&](uint64_t value){BeginProducerFrame();PublishProducerCapture(f.Get(),value);sync.arrive_and_wait();
        auto got=TakeProducerCapture();check(got.value==value,"another worker replaced this Present's receipt");
        check(!TakeProducerCapture().fence,"receipt consumed once");
        PublishProducerCapture(f.Get(),value);BeginProducerFrame();check(!TakeProducerCapture().fence,"failed capture cannot reuse stale receipt");};
    std::jthread one(worker,11),two(worker,22);one.join();two.join();

    // Real queued fence: do not drain unrelated future work, and do not hang on
    // a receipt which cannot complete. No device removal is induced.
    ComPtr<ID3D12CommandQueue> queue;D3D12_COMMAND_QUEUE_DESC desc{};hr(device->CreateCommandQueue(&desc,IID_PPV_ARGS(&queue)));
    auto done=fence(),gate=fence();hr(queue->Signal(done.Get(),1));hr(queue->Wait(gate.Get(),1));hr(queue->Signal(done.Get(),2));
    HANDLE timer{};auto wallNow=[]{return std::chrono::duration<double,std::milli>(std::chrono::steady_clock::now().time_since_epoch()).count();};
    auto pause=[&](double ms){return WaitProducerSlice(timer,ms);};
    result=GateProducer({done,1},8,wallNow,pause);
    check(result.result==R::Ready || result.result==R::Released,"exact receipt must pass even with later queued work blocked");
    check(done->GetCompletedValue()==1,"test queue gate did not remain blocked");
    result=GateProducer({done,2},3,wallNow,pause);
    check(result.result==R::Timeout && result.elapsedMs<1000,"blocked GPU must not hang Present");
    hr(gate->Signal(1));result=GateProducer({done,2},8,wallNow,pause);
    check(result.result==R::Ready || result.result==R::Released,"completed GPU resumes without stale callback");
    if(timer)CloseHandle(timer);
    ComPtr<ID3D12InfoQueue> info;if(SUCCEEDED(device.As(&info)))for(UINT64 i=0;i<info->GetNumStoredMessages();++i){
        SIZE_T size{};info->GetMessage(i,nullptr,&size);std::vector<char> bytes(size);auto* m=reinterpret_cast<D3D12_MESSAGE*>(bytes.data());info->GetMessage(i,m,&size);
        check(m->Severity>D3D12_MESSAGE_SEVERITY_ERROR,"D3D12 validation failure");
    }
    std::cout<<"PASS: bounded producer wait, ready bypass, exact queued receipt, failure recovery, worker isolation and refresh budgets\n";
}
