#include "Render/FoveatedGpu.hpp"
#include "Render/CommandResources.hpp"
#if !CVR_RUST_VRIK
// The C++ build's passes; the Rust build's are rust/crates/cvr-gpu/src/foveated_gpu.rs (the header calls them).
#include <d3dcompiler.h>
#include <cstring>
#include <algorithm>

extern void Log(const char*,...);
namespace cvr::foveated {
using Microsoft::WRL::ComPtr;
namespace {
constexpr char shader[] = R"(
Texture2D<float4> Color : register(t0);
Texture2D<float> Depth : register(t1);
Texture2D<float2> Motion : register(t2);
RWTexture2D<float4> ColorOut : register(u0);
RWTexture2D<float> DepthOut : register(u1);
RWTexture2D<float2> MotionOut : register(u2);
SamplerState LinearClamp : register(s0);
cbuffer Constants : register(b0) {
    float4 sourceRect;
    uint4 dimensions; // prepared input width/height, original render width/height
    uint4 bases; // original color and depth origins
    uint2 motionBase;
    float2 pixelScale;
    uint4 flags; // low-res motion, inverted depth, original output width/height
    uint4 region; // output x/y/width/height
}
[numthreads(8,8,1)] void Prepare(uint3 id : SV_DispatchThreadID) {
    if (any(id.xy >= dimensions.xy)) return;
    const float2 uv = (id.xy + 0.5) / dimensions.xy;
    const float2 pixel = sourceRect.xy + uv * sourceRect.zw;
    uint cw,ch; Color.GetDimensions(cw,ch);
    ColorOut[id.xy] = Color.SampleLevel(LinearClamp,(bases.xy+pixel)/float2(cw,ch),0);
    int2 p = clamp(int2(floor(pixel)),int2(0,0),int2(dimensions.zw)-1);
    float depth = Depth.Load(int3(int2(bases.zw)+p,0));
    // Downsampling selects a surface and its motion together at discontinuities.
    if (any(sourceRect.zw / dimensions.xy > 1.0)) {
        [unroll] for(int y=0;y<2;++y) [unroll] for(int x=0;x<2;++x) {
            int2 q=clamp(int2(floor(pixel-0.5))+int2(x,y),int2(0,0),int2(dimensions.zw)-1);
            float d=Depth.Load(int3(int2(bases.zw)+q,0));
            if(flags.y ? d>depth : d<depth) {depth=d;p=q;}
        }
    }
    DepthOut[id.xy] = depth;
    float2 mpos = (p+0.5) / dimensions.zw * (flags.x ? dimensions.zw : flags.zw);
    float2 motion = Motion.Load(int3(motionBase+uint2(mpos),0));
    MotionOut[id.xy] = motion * pixelScale;
}
Texture2D<float4> Center : register(t3);
Texture2D<float4> Peripheral : register(t4);
RWTexture2D<float4> Output : register(u3);
[numthreads(8,8,1)] void Composite(uint3 id : SV_DispatchThreadID) {
    if(any(id.xy >= dimensions.zw)) return;
    float2 uv = (id.xy+0.5)/dimensions.zw;
    float4 peripheral = Peripheral.SampleLevel(LinearClamp,uv,0);
    float2 local = float2(id.xy)+0.5-region.xy;
    float2 edge = min(local,float2(region.zw)-local);
    float blend = saturate(min(edge.x,edge.y)/max(1.0,0.04*min(region.z,region.w)));
    float4 value = peripheral;
    if(blend>0) {
        float2 sourcePosition=(id.xy+0.5)*dimensions.xy/dimensions.zw;
        float2 centerUV=(sourcePosition-sourceRect.xy)/sourceRect.zw;
        value=lerp(peripheral,Center.SampleLevel(LinearClamp,centerUV,0),blend);
    }
    if(flags.x && all(edge>=0) && min(edge.x,edge.y)<3) value=float4(1,0,0,1);
    Output[id.xy]=value;
}
)";
struct Constants {
    float rect[4]; unsigned dimensions[4],bases[4],motionBase[2]; float pixelScale[2];
    unsigned flags[4],region[4];
};
static_assert(sizeof(Constants)==96);
DXGI_FORMAT ViewFormat(DXGI_FORMAT format) {
    switch(format) {
    case DXGI_FORMAT_R32_TYPELESS: case DXGI_FORMAT_D32_FLOAT:return DXGI_FORMAT_R32_FLOAT;
    case DXGI_FORMAT_R24G8_TYPELESS: case DXGI_FORMAT_D24_UNORM_S8_UINT:return DXGI_FORMAT_R24_UNORM_X8_TYPELESS;
    case DXGI_FORMAT_R16G16_TYPELESS:return DXGI_FORMAT_R16G16_FLOAT;
    case DXGI_FORMAT_R16G16B16A16_TYPELESS:return DXGI_FORMAT_R16G16B16A16_FLOAT;
    case DXGI_FORMAT_R8G8B8A8_TYPELESS:return DXGI_FORMAT_R8G8B8A8_UNORM;
    default:return format;
    }
}
void Barrier(ID3D12GraphicsCommandList* list,ID3D12Resource* resource,D3D12_RESOURCE_STATES before,D3D12_RESOURCE_STATES after) {
    D3D12_RESOURCE_BARRIER b{}; b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition={resource,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,before,after};list->ResourceBarrier(1,&b);
}
bool Texture(ID3D12Device* device,unsigned w,unsigned h,DXGI_FORMAT format,ComPtr<ID3D12Resource>& result) {
    D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;d.Width=w;d.Height=h;
    d.DepthOrArraySize=1;d.MipLevels=1;d.SampleDesc.Count=1;d.Format=format;d.Flags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
    return SUCCEEDED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,nullptr,IID_PPV_ARGS(&result)));
}
bool Compile(ID3D12Device* device,ID3D12RootSignature* root,const char* entry,ComPtr<ID3D12PipelineState>& pso) {
    ComPtr<ID3DBlob> code,error;
    if(FAILED(D3DCompile(shader,sizeof(shader)-1,nullptr,nullptr,nullptr,entry,"cs_5_0",D3DCOMPILE_OPTIMIZATION_LEVEL3,0,&code,&error))) {
        Log("[foveated] %s: %s\n",entry,error?static_cast<const char*>(error->GetBufferPointer()):"compile failed");return false;
    }
    D3D12_COMPUTE_PIPELINE_STATE_DESC desc{};desc.pRootSignature=root;desc.CS={code->GetBufferPointer(),code->GetBufferSize()};
    return SUCCEEDED(device->CreateComputePipelineState(&desc,IID_PPV_ARGS(&pso)));
}
}
bool FoveatedGpu::Initialize(ID3D12Device* d,const Region& c,const Region& p) {
    device=d;center.region=c;periphery.region=p;stride=d->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    for(auto* pass : {&center,&periphery}) {
        const auto& r=pass->region;
        if(!Texture(d,r.inputW,r.inputH,DXGI_FORMAT_R16G16B16A16_FLOAT,pass->color) ||
           !Texture(d,r.inputW,r.inputH,DXGI_FORMAT_R32_FLOAT,pass->depth) ||
           !Texture(d,r.inputW,r.inputH,DXGI_FORMAT_R32G32_FLOAT,pass->motion) ||
           !Texture(d,r.width,r.height,DXGI_FORMAT_R16G16B16A16_FLOAT,pass->output))return false;
    }
    D3D12_DESCRIPTOR_RANGE ranges[2]{};
    ranges[0]={D3D12_DESCRIPTOR_RANGE_TYPE_SRV,5,0,0,0};ranges[1]={D3D12_DESCRIPTOR_RANGE_TYPE_UAV,4,0,0,0};
    D3D12_ROOT_PARAMETER params[3]{};
    for(unsigned i=0;i<2;++i){params[i].ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;params[i].DescriptorTable={1,&ranges[i]};}
    params[2].ParameterType=D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;params[2].Constants={0,0,sizeof(Constants)/4};
    D3D12_STATIC_SAMPLER_DESC sampler{};sampler.Filter=D3D12_FILTER_MIN_MAG_MIP_LINEAR;
    sampler.AddressU=sampler.AddressV=sampler.AddressW=D3D12_TEXTURE_ADDRESS_MODE_CLAMP;sampler.MaxLOD=D3D12_FLOAT32_MAX;
    D3D12_ROOT_SIGNATURE_DESC desc{};desc.NumParameters=3;desc.pParameters=params;desc.NumStaticSamplers=1;desc.pStaticSamplers=&sampler;
    ComPtr<ID3DBlob> blob,error;
    if(FAILED(D3D12SerializeRootSignature(&desc,D3D_ROOT_SIGNATURE_VERSION_1,&blob,&error)) ||
       FAILED(d->CreateRootSignature(0,blob->GetBufferPointer(),blob->GetBufferSize(),IID_PPV_ARGS(&root))))return false;
    return Compile(d,root.Get(),"Prepare",prepare) && Compile(d,root.Get(),"Composite",composite);
}
FoveatedGpu::Binding* FoveatedGpu::Bind(const Source& source) {
    for(auto& b:bindings)if(b.sources[0].Get()==source.color && b.sources[1].Get()==source.depth &&
        b.sources[2].Get()==source.motion && b.sources[3].Get()==source.output)return &b;
    Binding b;b.sources={source.color,source.depth,source.motion,source.output};
    D3D12_DESCRIPTOR_HEAP_DESC desc{};desc.Type=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
    desc.NumDescriptors=13;desc.Flags=D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
    if(FAILED(device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&b.heap))))return nullptr;
    auto cpu=b.heap->GetCPUDescriptorHandleForHeapStart();
    for(auto* texture:{source.color,source.depth,source.motion,center.output.Get(),periphery.output.Get()}) {
        D3D12_SHADER_RESOURCE_VIEW_DESC srv{};srv.Format=ViewFormat(texture->GetDesc().Format);srv.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2D;
        srv.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;srv.Texture2D.MipLevels=1;
        device->CreateShaderResourceView(texture,&srv,cpu);cpu.ptr+=stride;
    }
    for(auto* pass:{&center,&periphery}) for(auto* texture:{pass->color.Get(),pass->depth.Get(),pass->motion.Get(),source.output}) {
        D3D12_UNORDERED_ACCESS_VIEW_DESC uav{};uav.Format=ViewFormat(texture->GetDesc().Format);uav.ViewDimension=D3D12_UAV_DIMENSION_TEXTURE2D;
        device->CreateUnorderedAccessView(texture,nullptr,&uav,cpu);cpu.ptr+=stride;
    }
    if(bindings.size()>=8)bindings.erase(bindings.begin());
    bindings.push_back(std::move(b));return &bindings.back();
}
bool FoveatedGpu::Prepare(ID3D12GraphicsCommandList* list,const Source& s,bool central) {
    auto* binding=Bind(s);if(!binding)return false;
    auto& pass=central?center:periphery;const auto& r=pass.region;
    if(!cvr::gpu::KeepCommandResources(list,{binding->heap.Get(),s.color,s.depth,s.motion,s.output,root.Get(),prepare.Get(),
            pass.color.Get(),pass.depth.Get(),pass.motion.Get(),pass.output.Get()}))return false;
    Constants data{};
    // Periphery's source is the whole frame even when its private output is smaller.
    data.rect[0]=central?float(r.sourceX):0;data.rect[1]=central?float(r.sourceY):0;
    data.rect[2]=central?float(r.inputW):float(s.renderW);data.rect[3]=central?float(r.inputH):float(s.renderH);
    data.dimensions[0]=r.inputW;data.dimensions[1]=r.inputH;data.dimensions[2]=s.renderW;data.dimensions[3]=s.renderH;
    data.bases[0]=s.colorX;data.bases[1]=s.colorY;data.bases[2]=s.depthX;data.bases[3]=s.depthY;
    data.motionBase[0]=s.motionX;data.motionBase[1]=s.motionY;
    data.flags[0]=s.lowMotion;data.flags[1]=s.inverted;data.flags[2]=s.outputW;data.flags[3]=s.outputH;
    const float motionW=s.lowMotion?data.rect[2]:(central?float(r.width):float(s.outputW));
    const float motionH=s.lowMotion?data.rect[3]:(central?float(r.height):float(s.outputH));
    data.pixelScale[0]=s.motionScaleX*r.inputW/motionW;data.pixelScale[1]=s.motionScaleY*r.inputH/motionH;
    auto gpu=binding->heap->GetGPUDescriptorHandleForHeapStart();
    ID3D12DescriptorHeap* heaps[]{binding->heap.Get()};list->SetDescriptorHeaps(1,heaps);
    list->SetComputeRootSignature(root.Get());list->SetPipelineState(prepare.Get());
    list->SetComputeRootDescriptorTable(0,gpu);gpu.ptr+=uint64_t(central?5:9)*stride;
    list->SetComputeRootDescriptorTable(1,gpu);list->SetComputeRoot32BitConstants(2,sizeof(data)/4,&data,0);
    list->Dispatch((r.inputW+7)/8,(r.inputH+7)/8,1);
    for(auto* texture:{pass.color.Get(),pass.depth.Get(),pass.motion.Get()})
        Barrier(list,texture,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    return true;
}
bool FoveatedGpu::Composite(ID3D12GraphicsCommandList* list,const Source& s,bool boundary) {
    auto* binding=Bind(s);if(!binding || !cvr::gpu::KeepCommandResources(list,{binding->heap.Get(),s.output,root.Get(),composite.Get(),center.output.Get(),periphery.output.Get()}))return false;
    for(auto* texture:{center.output.Get(),periphery.output.Get()})
        Barrier(list,texture,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    Constants data{};data.dimensions[0]=s.renderW;data.dimensions[1]=s.renderH;
    data.dimensions[2]=s.outputW;data.dimensions[3]=s.outputH;data.flags[0]=boundary;
    data.rect[0]=float(center.region.sourceX);data.rect[1]=float(center.region.sourceY);
    data.rect[2]=float(center.region.inputW);data.rect[3]=float(center.region.inputH);
    data.region[0]=center.region.x;data.region[1]=center.region.y;data.region[2]=center.region.width;data.region[3]=center.region.height;
    ID3D12DescriptorHeap* heaps[]{binding->heap.Get()};list->SetDescriptorHeaps(1,heaps);
    list->SetComputeRootSignature(root.Get());list->SetPipelineState(composite.Get());
    auto gpu=binding->heap->GetGPUDescriptorHandleForHeapStart();list->SetComputeRootDescriptorTable(0,gpu);
    gpu.ptr+=uint64_t(5)*stride;list->SetComputeRootDescriptorTable(1,gpu);
    list->SetComputeRoot32BitConstants(2,sizeof(data)/4,&data,0);list->Dispatch((s.outputW+7)/8,(s.outputH+7)/8,1);
    D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_UAV;barrier.UAV.pResource=s.output;list->ResourceBarrier(1,&barrier);
    for(auto* pass:{&center,&periphery}) {
        Barrier(list,pass->output.Get(),D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
        for(auto* texture:{pass->color.Get(),pass->depth.Get(),pass->motion.Get()})
            Barrier(list,texture,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    }
    return true;
}
}
#endif
