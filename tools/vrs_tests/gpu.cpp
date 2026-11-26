#include "Render/FixedVrs.hpp"
#include "Render/CommandResources.hpp"
#include <wrl/client.h>
#include <dxgi1_6.h>
#include <d3d12sdklayers.h>
#include <d3dcompiler.h>
#include <chrono>
#include <thread>
#include <cstring>
#include <iostream>
#include <stdexcept>
#include <source_location>
using Microsoft::WRL::ComPtr;
using namespace cvr::vrs;
// Engine stereo is off here: the rate map is laid out for one eye per target.
namespace cvr::engine_stereo { bool Enabled(){return false;} }
#if CVR_RUST_VRIK
// The plugin defines the report's exports in Rust and hands them to the GPU half with the engine-stereo query as it loads
// (rust/crates/cvr-render/src/export_attach.rs); the test stands in for both, with the C++ definitions' values.
extern "C" {
__declspec(dllexport) uint32_t CyberpunkVR_FixedVrsReportSeq=0;
__declspec(dllexport) char CyberpunkVR_FixedVrsReport[1024]="{}";
}
namespace {
struct FixedVrsAttach {bool(*engineStereo)();uint32_t* reportSeq;char* report;size_t reportBytes;};
const FixedVrsAttach kFixedVrsAttach{[]{return cvr::engine_stereo::Enabled();},&CyberpunkVR_FixedVrsReportSeq,
    CyberpunkVR_FixedVrsReport,sizeof(CyberpunkVR_FixedVrsReport)};
}
extern "C" void cvr_vrs_attach(const FixedVrsAttach*);
#endif
static ComPtr<ID3D12InfoQueue> debugInfo;
static void Check(bool v,const char* text){if(!v)throw std::runtime_error(text);}
static void Hr(HRESULT h,const std::source_location where=std::source_location::current()){
    if(FAILED(h)){std::cerr<<where.line()<<": HRESULT 0x"<<std::hex<<unsigned(h)<<std::dec<<'\n';
        if(debugInfo)for(UINT64 i=0;i<debugInfo->GetNumStoredMessages();++i){SIZE_T bytes=0;debugInfo->GetMessage(i,nullptr,&bytes);std::vector<char> storage(bytes);auto* m=reinterpret_cast<D3D12_MESSAGE*>(storage.data());debugInfo->GetMessage(i,m,&bytes);std::cerr<<m->pDescription<<'\n';}
        throw std::runtime_error("D3D12 call failed");}
}
static D3D12_SHADING_RATE observedRate;
static std::array<D3D12_SHADING_RATE_COMBINER,2> observedCombiners;
static ID3D12Resource* observedImage{};
static ID3D12Resource* lastMap{};
static unsigned rateCalls{},imageCalls{};
static void STDMETHODCALLTYPE Rate(ID3D12GraphicsCommandList5* l,D3D12_SHADING_RATE r,const D3D12_SHADING_RATE_COMBINER* c){
    ++rateCalls;observedRate=r;observedCombiners=c?std::array{c[0],c[1]}:std::array{D3D12_SHADING_RATE_COMBINER_PASSTHROUGH,D3D12_SHADING_RATE_COMBINER_PASSTHROUGH};l->RSSetShadingRate(r,c);
}
static void STDMETHODCALLTYPE Image(ID3D12GraphicsCommandList5* l,ID3D12Resource* r){++imageCalls;observedImage=r;if(r)lastMap=r;l->RSSetShadingRateImage(r);}
static void Barrier(ID3D12GraphicsCommandList* l,ID3D12Resource* r,D3D12_RESOURCE_STATES from,D3D12_RESOURCE_STATES to){
    D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;b.Transition={r,0,from,to};l->ResourceBarrier(1,&b);
}
int main()try{
#if CVR_RUST_VRIK
    cvr_vrs_attach(&kFixedVrsAttach);
#endif
    ComPtr<ID3D12Debug> debug;Hr(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)));debug->EnableDebugLayer();
    ComPtr<IDXGIFactory6> factory;Hr(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));ComPtr<IDXGIAdapter1> adapter;
    Hr(factory->EnumAdapterByGpuPreference(0,DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,IID_PPV_ARGS(&adapter)));
    ComPtr<ID3D12Device> device;Hr(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_12_0,IID_PPV_ARGS(&device)));
    Hr(device.As(&debugInfo));
    D3D12_FEATURE_DATA_D3D12_OPTIONS6 options{};Hr(device->CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS6,&options,sizeof(options)));
    if(options.VariableShadingRateTier<D3D12_VARIABLE_SHADING_RATE_TIER_2){std::cout<<"SKIP hardware has no VRS Tier 2\n";return 0;}
    ComPtr<ID3D12CommandQueue> queue;D3D12_COMMAND_QUEUE_DESC q{};Hr(device->CreateCommandQueue(&q,IID_PPV_ARGS(&queue)));
    ComPtr<ID3D12Fence> fence;Hr(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence)));uint64_t serial=0;
    auto wait=[&]{Hr(queue->Signal(fence.Get(),++serial));HANDLE event=CreateEventW(nullptr,FALSE,FALSE,nullptr);Hr(fence->SetEventOnCompletion(serial,event));
        const auto result=WaitForSingleObject(event,10000);CloseHandle(event);Check(result==WAIT_OBJECT_0,"GPU completion timeout");};
    ComPtr<ID3D12CommandAllocator> allocator;Hr(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator)));
    ComPtr<ID3D12GraphicsCommandList> list;Hr(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&list)));
    ComPtr<ID3D12GraphicsCommandList5> list5;Hr(list.As(&list5));Hr(list->Close());
    const char* source=R"(
float4 vs(uint id:SV_VertexID):SV_Position { return float4(id==2?3:-1,id==1?3:-1,.5,1); }
float4 ps(float4 p:SV_Position):SV_Target { return float4(frac(p.x/31),frac(p.y/29),frac((p.x+p.y)/23),1); }
)";
    ComPtr<ID3DBlob> vs,ps,error;
    Hr(D3DCompile(source,std::strlen(source),nullptr,nullptr,nullptr,"vs","vs_5_0",D3DCOMPILE_OPTIMIZATION_LEVEL3,0,&vs,&error));
    Hr(D3DCompile(source,std::strlen(source),nullptr,nullptr,nullptr,"ps","ps_5_0",D3DCOMPILE_OPTIMIZATION_LEVEL3,0,&ps,&error));
    D3D12_ROOT_SIGNATURE_DESC rs{};ComPtr<ID3DBlob> blob;Hr(D3D12SerializeRootSignature(&rs,D3D_ROOT_SIGNATURE_VERSION_1,&blob,&error));
    ComPtr<ID3D12RootSignature> root;Hr(device->CreateRootSignature(0,blob->GetBufferPointer(),blob->GetBufferSize(),IID_PPV_ARGS(&root)));
    D3D12_GRAPHICS_PIPELINE_STATE_DESC pd{};pd.pRootSignature=root.Get();pd.VS={vs->GetBufferPointer(),vs->GetBufferSize()};pd.PS={ps->GetBufferPointer(),ps->GetBufferSize()};
    pd.BlendState.RenderTarget[0].RenderTargetWriteMask=15;pd.SampleMask=UINT_MAX;pd.SampleDesc.Count=1;
    for(auto& blend:pd.BlendState.RenderTarget){blend.SrcBlend=blend.SrcBlendAlpha=D3D12_BLEND_ONE;blend.DestBlend=blend.DestBlendAlpha=D3D12_BLEND_ZERO;
        blend.BlendOp=blend.BlendOpAlpha=D3D12_BLEND_OP_ADD;blend.LogicOp=D3D12_LOGIC_OP_NOOP;blend.RenderTargetWriteMask=15;}
    pd.RasterizerState.FillMode=D3D12_FILL_MODE_SOLID;pd.RasterizerState.CullMode=D3D12_CULL_MODE_NONE;pd.RasterizerState.DepthClipEnable=TRUE;
    pd.DepthStencilState.DepthEnable=TRUE;pd.DepthStencilState.DepthWriteMask=D3D12_DEPTH_WRITE_MASK_ALL;pd.DepthStencilState.DepthFunc=D3D12_COMPARISON_FUNC_ALWAYS;
    pd.PrimitiveTopologyType=D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;pd.NumRenderTargets=3;pd.DSVFormat=DXGI_FORMAT_D32_FLOAT;
    for(UINT i=0;i<pd.NumRenderTargets;++i)pd.RTVFormats[i]=DXGI_FORMAT_R8G8B8A8_UNORM;
    ComPtr<ID3D12PipelineState> pso;Hr(device->CreateGraphicsPipelineState(&pd,IID_PPV_ARGS(&pso)));GraphicsCreated(pso.Get(),pd);
    auto resource=[&](D3D12_RESOURCE_DESC d,D3D12_HEAP_TYPE type,D3D12_RESOURCE_STATES state){D3D12_HEAP_PROPERTIES h{};h.Type=type;ComPtr<ID3D12Resource> r;
        Hr(device->CreateCommittedResource(&h,D3D12_HEAP_FLAG_NONE,&d,state,nullptr,IID_PPV_ARGS(&r)));return r;};
    auto buffer=[&](UINT64 bytes){D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;d.Width=bytes;d.Height=d.DepthOrArraySize=d.MipLevels=d.SampleDesc.Count=1;d.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
        return resource(d,D3D12_HEAP_TYPE_READBACK,D3D12_RESOURCE_STATE_COPY_DEST);};
    constexpr UINT width=256,height=192;
    D3D12_RESOURCE_DESC texture{};texture.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;texture.Width=width;texture.Height=height;texture.DepthOrArraySize=texture.MipLevels=texture.SampleDesc.Count=1;
    texture.Format=DXGI_FORMAT_R8G8B8A8_UNORM;texture.Flags=D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    std::array<ComPtr<ID3D12Resource>,3> colors;for(auto& c:colors)c=resource(texture,D3D12_HEAP_TYPE_DEFAULT,D3D12_RESOURCE_STATE_RENDER_TARGET);
    texture.Format=DXGI_FORMAT_D32_FLOAT;texture.Flags=D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL;
    auto depth=resource(texture,D3D12_HEAP_TYPE_DEFAULT,D3D12_RESOURCE_STATE_DEPTH_WRITE);
    ComPtr<ID3D12DescriptorHeap> rtvs,dsvs;D3D12_DESCRIPTOR_HEAP_DESC hd{};hd.NumDescriptors=3;hd.Type=D3D12_DESCRIPTOR_HEAP_TYPE_RTV;Hr(device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&rtvs)));
    hd.NumDescriptors=1;hd.Type=D3D12_DESCRIPTOR_HEAP_TYPE_DSV;Hr(device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&dsvs)));
    auto rtv=rtvs->GetCPUDescriptorHandleForHeapStart();const auto stride=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
    for(unsigned i=0;i<3;++i)device->CreateRenderTargetView(colors[i].Get(),nullptr,{rtv.ptr+i*stride});
    auto dsv=dsvs->GetCPUDescriptorHandleForHeapStart();device->CreateDepthStencilView(depth.Get(),nullptr,dsv);
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT fp{};UINT64 bytes{};const auto colorDesc=colors[0]->GetDesc();device->GetCopyableFootprints(&colorDesc,0,1,0,&fp,nullptr,nullptr,&bytes);auto readback=buffer(bytes);
    ComPtr<ID3D12QueryHeap> queries;D3D12_QUERY_HEAP_DESC qd{};qd.Type=D3D12_QUERY_HEAP_TYPE_PIPELINE_STATISTICS;qd.Count=1;Hr(device->CreateQueryHeap(&qd,IID_PPV_ARGS(&queries)));
    auto counters=buffer(sizeof(D3D12_QUERY_DATA_PIPELINE_STATISTICS));
    D3D12_VIEWPORT viewport{0,0,float(width),float(height),0,1};D3D12_RECT scissor{0,0,width,height};
    auto begin=[&]{Hr(allocator->Reset());Hr(list->Reset(allocator.Get(),pso.Get()));cvr::gpu::ResetCommandResources(list.Get());Reset(list.Get(),true,pso.Get());
        list->SetGraphicsRootSignature(root.Get());list->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);list->RSSetViewports(1,&viewport);list->RSSetScissorRects(1,&scissor);
        Viewports(list.Get(),1,&viewport);list->OMSetRenderTargets(3,&rtv,TRUE,&dsv);Targets(list.Get(),3,true);
        Rate(list5.Get(),D3D12_SHADING_RATE_1X1,nullptr);Image(list5.Get(),nullptr);ShadingRate(list.Get(),D3D12_SHADING_RATE_1X1,nullptr);ShadingImage(list.Get(),nullptr);
    };
    SetSettings({1,70,65,0,0});std::this_thread::sleep_for(std::chrono::milliseconds(1010));Tick(device.Get(),queue.Get());
    begin();PrepareDraw(list.Get(),0x33DDA4,0,Rate,Image);Restore(list.Get());Hr(list->Close());
    Tick(device.Get(),queue.Get());wait();Tick(device.Get(),queue.Get());Check(GetStatus().readyMaps==1,"Map did not become ready");
    struct Result{std::vector<uint8_t> pixels;uint64_t invocations{};};
    auto render=[&](int mode,int eye,uint32_t node){
        begin();list->BeginQuery(queries.Get(),D3D12_QUERY_TYPE_PIPELINE_STATISTICS,0);
        if(mode==0)list->DrawInstanced(3,1,0,0);
        else {
            PrepareDraw(list.Get(),node,eye,Rate,Image);if(mode==1)list->DrawInstanced(3,1,0,0);
            const auto before=std::pair{rateCalls,imageCalls};
            PrepareDraw(list.Get(),node,eye,Rate,Image);PrepareDraw(list.Get(),node,eye,Rate,Image);
            Check(before==std::pair{rateCalls,imageCalls},"Repeated draws rebinding VRS");
            // A HUD/unsupported draw restores native state without a forced per-draw reset.
            PrepareDraw(list.Get(),0x111111,eye,Rate,Image);
            Check(observedRate==D3D12_SHADING_RATE_1X1 && observedCombiners[0]==D3D12_SHADING_RATE_COMBINER_PASSTHROUGH && observedCombiners[1]==D3D12_SHADING_RATE_COMBINER_PASSTHROUGH && !observedImage,"Native VRS state not restored");
            if(mode==2)list->DrawInstanced(3,1,0,0);
        }
        list->EndQuery(queries.Get(),D3D12_QUERY_TYPE_PIPELINE_STATISTICS,0);list->ResolveQueryData(queries.Get(),D3D12_QUERY_TYPE_PIPELINE_STATISTICS,0,1,counters.Get(),0);
        Barrier(list.Get(),colors[0].Get(),D3D12_RESOURCE_STATE_RENDER_TARGET,D3D12_RESOURCE_STATE_COPY_SOURCE);
        D3D12_TEXTURE_COPY_LOCATION from{},to{};from.pResource=colors[0].Get();to.pResource=readback.Get();to.Type=D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;to.PlacedFootprint=fp;
        list->CopyTextureRegion(&to,0,0,0,&from,nullptr);Barrier(list.Get(),colors[0].Get(),D3D12_RESOURCE_STATE_COPY_SOURCE,D3D12_RESOURCE_STATE_RENDER_TARGET);
        Hr(list->Close());ID3D12CommandList* lists[]{list.Get()};auto resources=cvr::gpu::PrepareCommandResources(1,lists);queue->ExecuteCommandLists(1,lists);cvr::gpu::SubmitCommandResources(queue.Get(),std::move(resources));wait();
        Result result;void* data{};Hr(readback->Map(0,nullptr,&data));result.pixels.resize(size_t(width)*height*4);
        for(UINT y=0;y<height;++y)std::memcpy(result.pixels.data()+y*width*4,static_cast<uint8_t*>(data)+fp.Offset+y*fp.Footprint.RowPitch,width*4);readback->Unmap(0,nullptr);
        Hr(counters->Map(0,nullptr,&data));result.invocations=static_cast<D3D12_QUERY_DATA_PIPELINE_STATISTICS*>(data)->PSInvocations;counters->Unmap(0,nullptr);return result;
    };
    const auto baseline=render(0,0,0),main=render(1,0,0x33DDA4),vr=render(1,1,0x33DDA4),restored=render(2,0,0x33DDA4),hud=render(1,0,0x111111);
    Check(main.pixels==vr.pixels,"Eye policies differ");Check(restored.pixels==baseline.pixels && hud.pixels==baseline.pixels,"VRS escaped scene scope");
    Check(main.invocations<baseline.invocations*.85,"Pixel shading did not decrease");
    const auto rates=BuildRates({1,70,65,0,0},width,height,options.ShadingRateImageTileSize,options.AdditionalShadingRatesSupported!=FALSE);
    unsigned differences=0;
    for(UINT y=0;y<height;++y)for(UINT x=0;x<width;++x){const auto offset=(size_t(y)*width+x)*4;const bool same=std::memcmp(baseline.pixels.data()+offset,main.pixels.data()+offset,4)==0;
        if(rates[size_t(y/options.ShadingRateImageTileSize)*((width+options.ShadingRateImageTileSize-1)/options.ShadingRateImageTileSize)+x/options.ShadingRateImageTileSize]==0)Check(same,"Fine center changed");
        differences+=!same;
    }
    Check(differences>0,"Peripheral shader rate did not affect interpolation");
    std::cout<<"PASS GPU VRS both eyes, exact center, restored/native HUD output; PS invocations "<<baseline.invocations<<" -> "<<main.invocations<<"\n";
    begin();
    Check(lastMap!=nullptr,"No hardware rate map bound");
    const auto* savedMap=lastMap;
    const D3D12_SHADING_RATE_COMBINER nativeCombiners[]{D3D12_SHADING_RATE_COMBINER_MAX,D3D12_SHADING_RATE_COMBINER_MIN};
    Rate(list5.Get(),D3D12_SHADING_RATE_2X1,nativeCombiners);Image(list5.Get(),lastMap);
    ShadingRate(list.Get(),D3D12_SHADING_RATE_2X1,nativeCombiners);ShadingImage(list.Get(),lastMap);
    PrepareDraw(list.Get(),0x33DDA4,0,Rate,Image);Restore(list.Get());
    Check(observedRate==D3D12_SHADING_RATE_2X1 && observedImage==savedMap && observedCombiners[0]==nativeCombiners[0] && observedCombiners[1]==nativeCombiners[1],"Non-default native state lost");
    Invalidate(list.Get());PrepareDraw(list.Get(),0x33DDA4,0,Rate,Image);
    Check(observedRate==D3D12_SHADING_RATE_2X1,"Unknown bundle state overridden");Hr(list->Close());
    SetSettings({2,40,40,15,10});std::this_thread::sleep_for(std::chrono::milliseconds(1010));Tick(device.Get(),queue.Get());
    begin();PrepareDraw(list.Get(),0x33DDA4,0,Rate,Image);Restore(list.Get());Hr(list->Close());Tick(device.Get(),queue.Get());wait();Tick(device.Get(),queue.Get());
    const auto strong=render(1,0,0x33DDA4);Check(strong.invocations<main.invocations,"Second configuration not applied");
    std::cout<<"PASS GPU native image/rate/combiners, unknown-state refusal and 4x4/settings rebuild; PS invocations "<<strong.invocations<<"\n";
    SetSettings({});const auto off=render(1,0,0x33DDA4);Check(off.pixels==baseline.pixels,"Off is not pass-through");
    cvr::gpu::CollectCommandResources();Shutdown();
    ComPtr<ID3D12InfoQueue> info;Hr(device.As(&info));
    for(UINT64 i=0;i<info->GetNumStoredMessages();++i){SIZE_T n=0;info->GetMessage(i,nullptr,&n);std::vector<char> storage(n);auto* m=reinterpret_cast<D3D12_MESSAGE*>(storage.data());Hr(info->GetMessage(i,m,&n));
        if(m->Severity<=D3D12_MESSAGE_SEVERITY_ERROR){std::cerr<<m->pDescription<<'\n';throw std::runtime_error("D3D12 validation error");}}
    return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
