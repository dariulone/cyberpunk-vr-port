#include "Render/MonoCascades.hpp"
#include "Render/CommandResources.hpp"
#if CVR_RUST_VRIK
#include "Render/RenderProbeScope.hpp"
#include "Stereo/EngineStereo.hpp"
#endif
#include <wrl/client.h>
#include <dxgi1_6.h>
#include <d3d12sdklayers.h>
#include <array>
#include <bit>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <iostream>
#include <stdexcept>
#include <vector>
using Microsoft::WRL::ComPtr;
using namespace cvr::cascades;
// The plugin's halves this test stands in for: the port's own stereo (engine stereo off, not a mono launch), so the
// second view is wanted; the Present count of the scene
// profile (Hooks/SwapChain.cpp).
namespace cvr::engine_stereo { bool Enabled(){return false;} bool MonoLaunch(){return false;} }
extern "C" std::atomic<uint64_t> CyberpunkVR_PresentTiming[8]{};
#if CVR_RUST_VRIK
// The plugin defines the cascades' exports in Rust and hands them over with its queries as it loads
// (rust/crates/cvr-render/src/export_attach.rs); the test stands in for both, with the C++ definitions' values.
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_MonoCascades{1};
__declspec(dllexport) uint32_t CyberpunkVR_MonoCascadeReportSeq=0;
__declspec(dllexport) char CyberpunkVR_MonoCascadeReport[4096]="{}";
void cvr_mono_cascades_attach(const void* table);
}
namespace {
bool SecondViewWanted(){return cvr::engine_stereo::SecondViewWanted();}
bool InternalCommands(){return cvr::stereo::probe::internalCommands;}
bool SetInternalCommands(bool on){const bool previous=cvr::stereo::probe::internalCommands;cvr::stereo::probe::internalCommands=on;return previous;}
struct CascadeAttach {
    std::atomic<uint32_t>* mono; std::atomic<uint32_t>* reportSeq; char* report; size_t reportLength;
    bool (*secondViewWanted)(); bool (*internalCommands)(); bool (*setInternalCommands)(bool);
};
const CascadeAttach kCascadeAttach{&CyberpunkVR_MonoCascades,reinterpret_cast<std::atomic<uint32_t>*>(&CyberpunkVR_MonoCascadeReportSeq),
    CyberpunkVR_MonoCascadeReport,sizeof(CyberpunkVR_MonoCascadeReport),&SecondViewWanted,&InternalCommands,&SetInternalCommands};
}
#endif
static void Check(bool v,const char* why){if(!v)throw std::runtime_error(why);}
static void Hr(HRESULT hr){Check(SUCCEEDED(hr),"D3D12 failed");}
// "trace": a hash of every atlas byte read back and the report, so the C++ and the Rust build compare
// (cmake/ParityTrace.cmake).
int main(int argc,char** argv)try{
#if CVR_RUST_VRIK
    cvr_mono_cascades_attach(&kCascadeAttach);
#endif
    const bool trace=argc>1 && std::strcmp(argv[1],"trace")==0;uint64_t hash=1469598103934665603ull;
    ComPtr<ID3D12Debug> debug;Hr(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)));debug->EnableDebugLayer();
    ComPtr<IDXGIFactory4> factory;Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));ComPtr<IDXGIAdapter> adapter;Hr(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter)));
    ComPtr<ID3D12Device> device;Hr(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_11_0,IID_PPV_ARGS(&device)));
    ComPtr<ID3D12CommandQueue> queue;D3D12_COMMAND_QUEUE_DESC q{};Hr(device->CreateCommandQueue(&q,IID_PPV_ARGS(&queue)));
    ComPtr<ID3D12CommandAllocator> allocator;Hr(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator)));
    ComPtr<ID3D12GraphicsCommandList> list;Hr(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&list)));Hr(list->Close());
    ComPtr<ID3D12Fence> fence;Hr(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)));uint64_t value=0;
    auto submit=[&]{Hr(list->Close());ID3D12CommandList* commands[]{list.Get()};queue->ExecuteCommandLists(1,commands);Hr(queue->Signal(fence.Get(),++value));
        HANDLE event=CreateEventW(nullptr,FALSE,FALSE,nullptr);Hr(fence->SetEventOnCompletion(value,event));const auto wait=WaitForSingleObject(event,10000);CloseHandle(event);Check(wait==WAIT_OBJECT_0,"GPU timeout");};
    std::array<float,64> matrices{};for(unsigned i=0;i<4;++i)for(unsigned j=0;j<4;++j)matrices[i*16+j*5]=1;
    std::array<uint8_t,848> camera{};CyberpunkVR_MonoCascades=1;
    for(UINT count:{2u,3u,4u,2u})for(bool mismatch:{false,true}) {
        const UINT width=count*32,height=count*32;
        D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;d.Width=width;d.Height=height;
        d.DepthOrArraySize=UINT16(count);d.MipLevels=1;d.SampleDesc.Count=1;d.Format=DXGI_FORMAT_R16_TYPELESS;d.Flags=D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL;
        D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;ComPtr<ID3D12Resource> atlas;
        Hr(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,D3D12_RESOURCE_STATE_DEPTH_WRITE,nullptr,IID_PPV_ARGS(&atlas)));
        ResourceCreated(atlas.Get(),nullptr,0,0);
        ComPtr<ID3D12DescriptorHeap> descriptors;D3D12_DESCRIPTOR_HEAP_DESC hd{};hd.Type=D3D12_DESCRIPTOR_HEAP_TYPE_DSV;hd.NumDescriptors=count+2;Hr(device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&descriptors)));
        const auto start=descriptors->GetCPUDescriptorHandleForHeapStart();const auto stride=device->GetDescriptorHandleIncrementSize(hd.Type);
        auto handle=[&](UINT n){return D3D12_CPU_DESCRIPTOR_HANDLE{start.ptr+size_t(n)*stride};};
        D3D12_DEPTH_STENCIL_VIEW_DESC v{};v.Format=DXGI_FORMAT_D16_UNORM;v.ViewDimension=D3D12_DSV_DIMENSION_TEXTURE2DARRAY;v.Texture2DArray.ArraySize=count;
        device->CreateDepthStencilView(atlas.Get(),&v,handle(0));DsvCreated(atlas.Get(),&v,handle(0));
        for(UINT i=0;i<count;++i){v.Texture2DArray.FirstArraySlice=i;v.Texture2DArray.ArraySize=1;device->CreateDepthStencilView(atlas.Get(),&v,handle(i+1));DsvCreated(atlas.Get(),&v,handle(i+1));}
        const auto copied=handle(count+1),first=handle(0);const UINT one=1;
        device->CopyDescriptorsSimple(1,copied,first,D3D12_DESCRIPTOR_HEAP_TYPE_DSV);CopyDescriptors(device.Get(),1,&copied,&one,1,&first,&one,D3D12_DESCRIPTOR_HEAP_TYPE_DSV);
        Hr(allocator->Reset());Hr(list->Reset(allocator.Get(),nullptr));Reset(list.Get());cvr::gpu::ResetCommandResources(list.Get());SourceSampling(matrices.data(),count);
        Check(!Clear(list.Get(),copied,D3D12_CLEAR_FLAG_DEPTH,1,0,0,1,true),"Source clear suppressed");list->ClearDepthStencilView(copied,D3D12_CLEAR_FLAG_DEPTH,1,0,0,nullptr);
        for(UINT i=0;i<count;++i){BeginNode(1,int(i));Camera(i,camera.data(),camera.size(),true);const auto h=handle(i+1);Targets(list.Get(),0,&h);
            Check(!SkipDraw(list.Get()),"Source raster suppressed");list->ClearDepthStencilView(h,D3D12_CLEAR_FLAG_DEPTH,.25f+i*.125f,0,0,nullptr);
            Targets(list.Get(),0,nullptr);EndNode();
            D3D12_RESOURCE_BARRIER alias{};alias.Type=D3D12_RESOURCE_BARRIER_TYPE_ALIASING;
            BeforeBarriers(list.Get(),1,&alias);list->ResourceBarrier(1,&alias);Barriers(1,&alias);
        }
        Close(list.Get());
        // Native transient contents are deliberately destroyed between the eyes.
        Write(atlas.Get());list->ClearDepthStencilView(copied,D3D12_CLEAR_FLAG_DEPTH,.9f,0,0,nullptr);
        MainSampling(matrices.data(),count);Check(Clear(list.Get(),copied,D3D12_CLEAR_FLAG_DEPTH,1,0,0,0,true),"Current complete atlas not reused");
        for(UINT i=0;i<count;++i){if(mismatch && i==count-1){auto different=matrices;different[0]=2;MainSampling(different.data(),count);}
            const bool skipped=SkipNode(0,int(i));Check(skipped==!(mismatch && i==count-1),"Invalid node still reused");
            if(skipped)continue;
            BeginNode(0,int(i));Camera(i,camera.data(),camera.size(),false);const auto h=handle(i+1);Targets(list.Get(),0,&h);
            Check(!SkipDraw(list.Get()),"Fallback draw suppressed");EndNode();}
        std::vector<D3D12_PLACED_SUBRESOURCE_FOOTPRINT> footprints(count);UINT64 bytes{};device->GetCopyableFootprints(&d,0,count,0,footprints.data(),nullptr,nullptr,&bytes);
        D3D12_RESOURCE_DESC bd{};bd.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;bd.Width=bytes;bd.Height=bd.DepthOrArraySize=bd.MipLevels=1;bd.SampleDesc.Count=1;bd.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        heap.Type=D3D12_HEAP_TYPE_READBACK;ComPtr<ID3D12Resource> readback;Hr(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&bd,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&readback)));
        D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;barrier.Transition={atlas.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_STATE_COPY_SOURCE};list->ResourceBarrier(1,&barrier);
        for(UINT i=0;i<count;++i){D3D12_TEXTURE_COPY_LOCATION from{},to{};from.pResource=atlas.Get();from.SubresourceIndex=i;to.pResource=readback.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=footprints[i];list->CopyTextureRegion(&to,0,0,0,&from,nullptr);}
        submit();void* data{};Hr(readback->Map(0,nullptr,&data));
        for(size_t b=0;b<bytes;++b){hash^=static_cast<const uint8_t*>(data)[b];hash*=1099511628211ull;}
        for(UINT i=0;i<count;++i)for(UINT y=0;y<height;++y)for(UINT x=0;x<width;++x){const auto* row=reinterpret_cast<const uint16_t*>(static_cast<const uint8_t*>(data)+footprints[i].Offset+y*footprints[i].Footprint.RowPitch);
            const auto expected=mismatch && i==count-1?65535u:unsigned(std::lround((.25f+i*.125f)*65535));Check(std::abs(int(row[x])-int(expected))<=1,"Atlas layer lost after quality transition or fallback");}
        readback->Unmap(0,nullptr);
    }
    Poll();
    if(trace){std::printf("cascade_gpu trace %016llx %s\n",static_cast<unsigned long long>(hash),CyberpunkVR_MonoCascadeReport);return 0;}
    CyberpunkVR_MonoCascades=0;ComPtr<ID3D12InfoQueue> info;Hr(device.As(&info));
    for(UINT64 i=0;i<info->GetNumStoredMessages();++i){SIZE_T bytes{};info->GetMessage(i,nullptr,&bytes);std::vector<char> storage(bytes);auto* m=reinterpret_cast<D3D12_MESSAGE*>(storage.data());info->GetMessage(i,m,&bytes);
        if(m->Severity<=D3D12_MESSAGE_SEVERITY_ERROR){std::cerr<<m->pDescription<<'\n';throw std::runtime_error("GPU validation error");}}
    std::cout<<"PASS GPU dynamic atlas sizes, 2/3/4/2 cascades, copied DSVs, preserved MAIN depth and per-slice invalidation fallback\n";
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
