#include "Render/FoveatedGpu.hpp"
#include "Render/CommandResources.hpp"
#include <dxgi1_6.h>
#include <d3d12sdklayers.h>
#include <cstdarg>
#include <cstdio>
#include <cmath>
#include <cstring>
#include <iostream>
#include <stdexcept>
using Microsoft::WRL::ComPtr;
void Log(const char* f,...){va_list args;va_start(args,f);vprintf(f,args);va_end(args);}
void Check(bool value,const char* text){if(!value)throw std::runtime_error(text);}
void Hr(HRESULT value){Check(SUCCEEDED(value),"D3D12 failure");}
void Barrier(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,D3D12_RESOURCE_STATES from,D3D12_RESOURCE_STATES to){
    D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;b.Transition={resource,0,from,to};list->ResourceBarrier(1,&b);}
// "trace" prints a hash of every byte read back (the prepared crops, both motions and the composite), so the C++ and
// the Rust build compare bit for bit (cmake/ParityTrace.cmake).
int main(int argc,char** argv)try{
    const bool trace=argc>1 && std::strcmp(argv[1],"trace")==0;uint64_t hash=1469598103934665603ull;
    ComPtr<ID3D12Debug> debug;Hr(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)));debug->EnableDebugLayer();
    ComPtr<IDXGIFactory4> factory;Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));ComPtr<IDXGIAdapter> adapter;Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)));
    ComPtr<ID3D12Device> device;Hr(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&device)));
    ComPtr<ID3D12CommandQueue> queue;D3D12_COMMAND_QUEUE_DESC q{};Hr(device->CreateCommandQueue(&q,IID_PPV_ARGS(&queue)));
    ComPtr<ID3D12CommandAllocator> allocator;Hr(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator)));
    ComPtr<ID3D12GraphicsCommandList> list;Hr(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&list)));
    auto texture=[&](unsigned w,unsigned h,DXGI_FORMAT format,D3D12_RESOURCE_STATES state){
        D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;d.Width=w;d.Height=h;d.DepthOrArraySize=d.MipLevels=d.SampleDesc.Count=1;d.Format=format;d.Flags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
        D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;ComPtr<ID3D12Resource> r;Hr(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,state,nullptr,IID_PPV_ARGS(&r)));return r;};
    auto buffer=[&](UINT64 bytes,D3D12_HEAP_TYPE type,D3D12_RESOURCE_STATES state){
        D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;d.Width=bytes;d.Height=d.DepthOrArraySize=d.MipLevels=d.SampleDesc.Count=1;d.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        D3D12_HEAP_PROPERTIES heap{};heap.Type=type;ComPtr<ID3D12Resource> r;Hr(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,state,nullptr,IID_PPV_ARGS(&r)));return r;};
    std::vector<ComPtr<ID3D12Resource>> uploads;
    auto upload=[&](ID3D12Resource* image,unsigned components){
        const auto d=image->GetDesc();D3D12_PLACED_SUBRESOURCE_FOOTPRINT fp{};UINT64 bytes{};device->GetCopyableFootprints(&d,0,1,0,&fp,nullptr,nullptr,&bytes);
        auto data=buffer(bytes,D3D12_HEAP_TYPE_UPLOAD,D3D12_RESOURCE_STATE_GENERIC_READ);void* p{};Hr(data->Map(0,nullptr,&p));
        for(unsigned y=0;y<d.Height;++y)for(unsigned x=0;x<d.Width;++x){auto* v=reinterpret_cast<float*>(static_cast<char*>(p)+y*fp.Footprint.RowPitch)+x*components;
            if(components==4){v[0]=.25f+x/256.0f;v[1]=y/256.0f;v[2]=.5f;v[3]=1;}
            else if(components==2){v[0]=.125f;v[1]=-.0625f;}else v[0]=.5f+x/256.0f+y/512.0f;}
        data->Unmap(0,nullptr);D3D12_TEXTURE_COPY_LOCATION from{},to{};from.pResource=data.Get();from.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;from.PlacedFootprint=fp;to.pResource=image;
        list->CopyTextureRegion(&to,0,0,0,&from,nullptr);Barrier(list.Get(),image,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);uploads.push_back(data);};
    auto color=texture(64,48,DXGI_FORMAT_R32G32B32A32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
    auto depth=texture(64,48,DXGI_FORMAT_R32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
    auto motion=texture(64,48,DXGI_FORMAT_R32G32_FLOAT,D3D12_RESOURCE_STATE_COPY_DEST);
    auto output=texture(128,96,DXGI_FORMAT_R32G32B32A32_FLOAT,D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    upload(color.Get(),4);upload(depth.Get(),1);upload(motion.Get(),2);
    cvr::foveated::Source source{color.Get(),depth.Get(),motion.Get(),output.Get(),64,48,128,96,0,0,0,0,0,0,64,48,true,true};
    cvr::foveated::FoveatedGpu gpu;
    Check(gpu.Initialize(device.Get(),{16,48,64,48,32,24,8,24},{0,0,32,24,32,24,0,0}),"shader/resource initialization");
    Check(gpu.Prepare(list.Get(),source,true),"center preparation");Check(gpu.Prepare(list.Get(),source,false),"periphery preparation");
    struct Readback{ComPtr<ID3D12Resource> buffer;D3D12_PLACED_SUBRESOURCE_FOOTPRINT fp;};
    auto read=[&](ID3D12Resource* image){const auto d=image->GetDesc();Readback r{};UINT64 bytes{};device->GetCopyableFootprints(&d,0,1,0,&r.fp,nullptr,nullptr,&bytes);
        r.buffer=buffer(bytes,D3D12_HEAP_TYPE_READBACK,D3D12_RESOURCE_STATE_COPY_DEST);Barrier(list.Get(),image,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_COPY_SOURCE);
        D3D12_TEXTURE_COPY_LOCATION from{},to{};from.pResource=image;to.pResource=r.buffer.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=r.fp;
        list->CopyTextureRegion(&to,0,0,0,&from,nullptr);Barrier(list.Get(),image,D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);return r;};
    auto centerDepth=read(gpu.center.depth.Get()),centerMotion=read(gpu.center.motion.Get()),peripheralMotion=read(gpu.periphery.motion.Get()),centerColor=read(gpu.center.color.Get());
    ComPtr<ID3D12DescriptorHeap> clearHeap;D3D12_DESCRIPTOR_HEAP_DESC hd{};hd.NumDescriptors=2;
    hd.Type=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;hd.Flags=D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    Hr(device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&clearHeap)));ID3D12DescriptorHeap* heaps[]{clearHeap.Get()};list->SetDescriptorHeaps(1,heaps);
    ComPtr<ID3D12DescriptorHeap> clearCpuHeap;hd.Flags=D3D12_DESCRIPTOR_HEAP_FLAG_NONE;Hr(device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&clearCpuHeap)));
    auto cpu=clearCpuHeap->GetCPUDescriptorHandleForHeapStart();auto visibleCpu=clearHeap->GetCPUDescriptorHandleForHeapStart();
    auto gpuHandle=clearHeap->GetGPUDescriptorHandleForHeapStart();
    const auto stride=device->GetDescriptorHandleIncrementSize(hd.Type);unsigned side=0;
    for(auto* image:{gpu.center.output.Get(),gpu.periphery.output.Get()}) {
        D3D12_UNORDERED_ACCESS_VIEW_DESC view{};view.Format=DXGI_FORMAT_R16G16B16A16_FLOAT;view.ViewDimension=D3D12_UAV_DIMENSION_TEXTURE2D;
        device->CreateUnorderedAccessView(image,nullptr,&view,cpu);device->CreateUnorderedAccessView(image,nullptr,&view,visibleCpu);
        const float value[]{side?0.0f:1.0f,0,side?1.0f:0.0f,1};
        list->ClearUnorderedAccessViewFloat(gpuHandle,cpu,image,value,0,nullptr);cpu.ptr+=stride;visibleCpu.ptr+=stride;gpuHandle.ptr+=stride;++side;
    }
    Check(gpu.Composite(list.Get(),source,false),"composite");
    Barrier(list.Get(),output.Get(),D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    auto composite=read(output.Get());
    Hr(list->Close());ID3D12CommandList* lists[]{list.Get()};auto resources=cvr::gpu::PrepareCommandResources(1,lists);
    queue->ExecuteCommandLists(1,lists);cvr::gpu::SubmitCommandResources(queue.Get(),std::move(resources));
    ComPtr<ID3D12Fence> fence;Hr(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)));Hr(queue->Signal(fence.Get(),1));
    const auto event=CreateEventW(nullptr,FALSE,FALSE,nullptr);Hr(fence->SetEventOnCompletion(1,event));Check(WaitForSingleObject(event,5000)==WAIT_OBJECT_0,"GPU timeout");CloseHandle(event);
    for(auto* r:{&centerDepth,&centerMotion,&peripheralMotion,&centerColor,&composite}){
        void* p{};Hr(r->buffer->Map(0,nullptr,&p));const auto& f=r->fp.Footprint;
        for(unsigned y=0;y<f.Height;++y){const auto* row=static_cast<const uint8_t*>(p)+y*f.RowPitch;
            for(unsigned i=0;i<f.RowPitch;++i){hash^=row[i];hash*=1099511628211ull;}}
        r->buffer->Unmap(0,nullptr);
    }
    for(auto* r:{&centerDepth,&centerMotion,&peripheralMotion,&centerColor}){
        void* p{};Hr(r->buffer->Map(0,nullptr,&p));
        for(unsigned y=0;y<24;++y)for(unsigned x=0;x<32;++x){const auto* row=static_cast<const char*>(p)+y*r->fp.Footprint.RowPitch;
            if(r==&centerDepth)Check(std::abs(reinterpret_cast<const float*>(row)[x]-(.5f+(x+8)/256.0f+(y+24)/512.0f))<1e-6f,"offset depth crop mismatch");
            else if(r==&centerColor)Check(reinterpret_cast<const uint16_t*>(row)[x*4]==uint16_t(0x3400+(x+8)*16),"offset color crop was filtered or shifted");
            else {const auto* value=reinterpret_cast<const float*>(row)+x*2;const float scale=r==&centerMotion?1.0f:.5f;
                Check(value[0]==8*scale && value[1]==-3*scale,"motion displacement scale mismatch");}}
        r->buffer->Unmap(0,nullptr);
    }
    void* composed{};Hr(composite.buffer->Map(0,nullptr,&composed));
    for(unsigned y=0;y<96;++y)for(unsigned x=0;x<128;++x) {
        const auto* p=reinterpret_cast<const float*>(static_cast<const char*>(composed)+y*composite.fp.Footprint.RowPitch)+x*4;
        if(x>=24 && x<72 && y>=56 && y<88)Check(p[0]==1 && p[2]==0,"shifted center composite not preserved");
        if(x<16 || x>=80 || y<48)Check(p[0]==0 && p[2]==1,"peripheral composite not preserved");
        Check(std::isfinite(p[0]) && p[3]==1,"invalid composite pixel");
    }
    composite.buffer->Unmap(0,nullptr);
    ComPtr<ID3D12InfoQueue> info;Hr(device.As(&info));for(UINT64 i=0;i<info->GetNumStoredMessages();++i){SIZE_T n{};info->GetMessage(i,nullptr,&n);std::vector<char> b(n);auto* m=reinterpret_cast<D3D12_MESSAGE*>(b.data());info->GetMessage(i,m,&n);
        if(m->Severity<=D3D12_MESSAGE_SEVERITY_WARNING){std::cerr<<m->pDescription<<'\n';throw std::runtime_error("D3D12 validation");}}
    if(trace){std::printf("foveated_gpu trace %016llx\n",static_cast<unsigned long long>(hash));return 0;}
    std::cout<<"PASS GPU center color/depth crop, per-region pixel-motion scaling and full output composition\n";return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
