#include "Framegen/SynthesisQueue.hpp"
#include <dxgi1_6.h>
#include <d3d12sdklayers.h>
#include <windows.h>
#include <vector>
#include <array>
#include <iostream>
#include <cstdlib>
using Microsoft::WRL::ComPtr;
using cvr::framegen::SynthesisQueue;
void check(bool ok,const char* text){if(!ok){std::cerr<<text<<'\n';std::exit(1);}}
void hr(HRESULT h,const char* text){if(FAILED(h)){std::cerr<<text<<" hr="<<std::hex<<h<<'\n';std::exit(1);}}
ComPtr<ID3D12Resource> buffer(ID3D12Device* d,D3D12_HEAP_TYPE heap,UINT64 bytes,D3D12_RESOURCE_STATES state){
    D3D12_HEAP_PROPERTIES hp{};hp.Type=heap;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=bytes;
    desc.Height=1;desc.DepthOrArraySize=1;desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ComPtr<ID3D12Resource> r;hr(d->CreateCommittedResource(&hp,D3D12_HEAP_FLAG_NONE,&desc,state,nullptr,IID_PPV_ARGS(&r)),"buffer");return r;
}
void barrier(ID3D12GraphicsCommandList* l,ID3D12Resource* r,D3D12_RESOURCE_STATES a,D3D12_RESOURCE_STATES b){
    D3D12_RESOURCE_BARRIER v{};v.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;v.Transition={r,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,a,b};l->ResourceBarrier(1,&v);
}
struct Commands {
    ComPtr<ID3D12CommandAllocator> a;ComPtr<ID3D12GraphicsCommandList> l;
    Commands(ID3D12Device* d){hr(d->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&a)),"allocator");hr(d->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,a.Get(),nullptr,IID_PPV_ARGS(&l)),"list");}
    void submit(ID3D12CommandQueue* q){hr(l->Close(),"close");ID3D12CommandList* ls[]{l.Get()};q->ExecuteCommandLists(1,ls);}
};
void awaitFence(ID3D12Fence* f,uint64_t v){
    HANDLE e=CreateEventW(nullptr,FALSE,FALSE,nullptr);check(e!=nullptr,"event");
    hr(f->SetEventOnCompletion(v,e),"set completion");check(WaitForSingleObject(e,5000)==WAIT_OBJECT_0,"GPU queue graph must complete without deadlock");CloseHandle(e);
}
void run(ID3D12Device* d,bool isolated,bool fallback){
    D3D12_COMMAND_QUEUE_DESC qd{};qd.Type=D3D12_COMMAND_LIST_TYPE_DIRECT;
    ComPtr<ID3D12CommandQueue> producer;hr(d->CreateCommandQueue(&qd,IID_PPV_ARGS(&producer)),"producer");
    SynthesisQueue route;check(route.Initialize(d,producer.Get(),isolated),"initialize");
    check(route.Private()==isolated,"requested isolation must be exercised on test GPU");
    if(isolated)check(route.Queue()->GetDesc().Priority==D3D12_COMMAND_QUEUE_PRIORITY_HIGH,"V557 HIGH priority");
    else check(route.Queue()==producer.Get(),"VDXR/shared path preserves original queue");
    ComPtr<ID3D12Fence> capture,output,consumer,gate;
    hr(d->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&capture)),"capture fence");
    hr(d->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&output)),"output fence");
    hr(d->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&consumer)),"consumer fence");
    hr(d->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&gate)),"gate fence");
    constexpr unsigned n=24,bytes=256;
    auto upload=buffer(d,D3D12_HEAP_TYPE_UPLOAD,n*bytes,D3D12_RESOURCE_STATE_GENERIC_READ);
    auto source=buffer(d,D3D12_HEAP_TYPE_DEFAULT,bytes,D3D12_RESOURCE_STATE_COPY_DEST);
    auto generated=buffer(d,D3D12_HEAP_TYPE_DEFAULT,bytes,D3D12_RESOURCE_STATE_COPY_DEST);
    auto readback=buffer(d,D3D12_HEAP_TYPE_READBACK,n*bytes,D3D12_RESOURCE_STATE_COPY_DEST);
    void* ptr{};hr(upload->Map(0,nullptr,&ptr),"map upload");
    for(unsigned i=0;i<n;++i)for(unsigned j=0;j<bytes/4;++j)static_cast<uint32_t*>(ptr)[i*bytes/4+j]=0xCAFE0000+i*97+j;
    upload->Unmap(0,nullptr);
    std::vector<Commands> commands;commands.reserve(n*3);
    // Delay the producer deliberately. All 24 cycles are enqueued before
    // releasing the gate, exposing dependency cycles and output reuse races.
    hr(producer->Wait(gate.Get(),1),"gate producer");
    for(unsigned i=0;i<n;++i){
        commands.emplace_back(d);auto& a=commands.back();
        if(i)barrier(a.l.Get(),source.Get(),D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_COPY_DEST);
        a.l->CopyBufferRegion(source.Get(),0,upload.Get(),i*bytes,bytes);
        barrier(a.l.Get(),source.Get(),D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_COPY_SOURCE);a.submit(producer.Get());
        hr(producer->Signal(capture.Get(),i+1),"capture signal");
        check(route.Begin(fallback?nullptr:capture.Get(),fallback?0:i+1,consumer.Get(),i),"input/previous consumer wait");
        check(route.WaitInput(capture.Get(),i+1),"explicit depth/motion dependency");
        commands.emplace_back(d);auto& b=commands.back();
        if(i)barrier(b.l.Get(),generated.Get(),D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_COPY_DEST);
        b.l->CopyBufferRegion(generated.Get(),0,source.Get(),0,bytes);
        barrier(b.l.Get(),generated.Get(),D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_COPY_SOURCE);b.submit(route.Queue());
        hr(route.Queue()->Signal(output.Get(),i+1),"synthesis signal");
        check(route.JoinOutput(output.Get(),i+1),"output join");
        commands.emplace_back(d);auto& c=commands.back();c.l->CopyBufferRegion(readback.Get(),i*bytes,generated.Get(),0,bytes);c.submit(producer.Get());
        hr(producer->Signal(consumer.Get(),i+1),"consumer signal");
    }
    check(output->GetCompletedValue()==0,"synthesis cannot read before delayed input becomes ready");
    hr(gate->Signal(1),"release gate");awaitFence(consumer.Get(),n);
    D3D12_RANGE range{0,n*bytes};hr(readback->Map(0,&range,&ptr),"readback");
    for(unsigned i=0;i<n;++i)for(unsigned j=0;j<bytes/4;++j)check(static_cast<uint32_t*>(ptr)[i*bytes/4+j]==0xCAFE0000+i*97+j,"ordered output data / no premature overwrite");
    D3D12_RANGE none{};readback->Unmap(0,&none);
}
int main(){
    ComPtr<ID3D12Debug> debug;if(SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug))))debug->EnableDebugLayer();
    ComPtr<IDXGIFactory4> factory;hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)),"DXGI");
    ComPtr<ID3D12Device> device;
    for(UINT i=0;!device;++i){ComPtr<IDXGIAdapter1> adapter;if(factory->EnumAdapters1(i,&adapter)==DXGI_ERROR_NOT_FOUND)break;
        DXGI_ADAPTER_DESC1 desc{};adapter->GetDesc1(&desc);if(!(desc.Flags&DXGI_ADAPTER_FLAG_SOFTWARE))D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&device));}
    check(device!=nullptr,"hardware D3D12 adapter required for HIGH queue test");
    ComPtr<ID3D12InfoQueue> info;device.As(&info);if(info)info->ClearStoredMessages();
    run(device.Get(),false,false);run(device.Get(),true,false);run(device.Get(),true,true);
    // Recreate queues after teardown as switching FG backend/session does.
    run(device.Get(),true,false);run(device.Get(),false,false);
    if(info)for(UINT64 i=0;i<info->GetNumStoredMessages();++i){SIZE_T size{};info->GetMessage(i,nullptr,&size);std::vector<char> data(size);auto* m=reinterpret_cast<D3D12_MESSAGE*>(data.data());info->GetMessage(i,m,&size);if(m->Severity<=D3D12_MESSAGE_SEVERITY_ERROR){std::cerr<<m->pDescription<<'\n';check(false,"D3D12 validation error");}}
    std::cout<<"PASS: real GPU HIGH queue, 24-frame delayed producer, explicit/fallback input fences, output join/reuse, shared bypass and recreation; debug="<<(info?1:0)<<'\n';
}
