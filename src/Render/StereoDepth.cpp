#include "Render/StereoDepth.hpp"
#include <d3dcompiler.h>
#include <array>
#include <cstring>

namespace cvr::depth {
namespace {
using Microsoft::WRL::ComPtr;
constexpr auto Read=D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
constexpr auto Copy=D3D12_RESOURCE_STATE_COPY_SOURCE;
constexpr auto Uav=D3D12_RESOURCE_STATE_UNORDERED_ACCESS;
constexpr char shader[]=R"(
Texture2D<float> sourceDepth:register(t0);
RWTexture2D<float> outputDepth:register(u0);
cbuffer Parameters:register(b0) {uint2 size;float2 jitter;float nearPlane;float farPlane;uint inverted;uint unused;};
float clean(float d){return isfinite(d) && d>=0 && d<=1 ? d : (inverted?0:1);}
float distanceAt(float d){
    if(inverted)d=1-d;
    if(isinf(farPlane))return nearPlane/max(1-d,1e-20);
    return nearPlane*farPlane/max(farPlane-d*(farPlane-nearPlane),1e-20);
}
[numthreads(8,8,1)] void main(uint3 id:SV_DispatchThreadID) {
    if(any(id.xy>=size))return;
    // Native 0xC28EC0 adds +2*jitterX/W,+2*jitterY/H to NDC. CameraData
    // already flips Y for texture coordinates. Sample the rasterized depth
    // at that offset to describe the unjittered color/projection sent to XR.
    float2 p=float2(id.xy)+jitter;int2 b=int2(floor(p));float2 w=frac(p);
    float4 d=float4(clean(sourceDepth.Load(int3(clamp(b,int2(0,0),int2(size)-1),0))),
        clean(sourceDepth.Load(int3(clamp(b+int2(1,0),int2(0,0),int2(size)-1),0))),
        clean(sourceDepth.Load(int3(clamp(b+int2(0,1),int2(0,0),int2(size)-1),0))),
        clean(sourceDepth.Load(int3(clamp(b+int2(1,1),int2(0,0),int2(size)-1),0))));
    float4 z=float4(distanceAt(d.x),distanceAt(d.y),distanceAt(d.z),distanceAt(d.w));
    float lo=min(min(z.x,z.y),min(z.z,z.w)),hi=max(max(z.x,z.y),max(z.z,z.w));
    // Device depth is affine over a planar surface. Interpolate it there;
    // never blend foreground and background into an invented surface at an edge.
    float value=hi<=lo*1.05 ? lerp(lerp(d.x,d.y,w.x),lerp(d.z,d.w,w.x),w.y)
        : clean(sourceDepth.Load(int3(clamp(int2(floor(p+.5)),int2(0,0),int2(size)-1),0)));
    outputDepth[id.xy]=value;
})";
void Barrier(ID3D12GraphicsCommandList* l,ID3D12Resource* r,D3D12_RESOURCE_STATES a,D3D12_RESOURCE_STATES b) {
    D3D12_RESOURCE_BARRIER x{};x.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    x.Transition={r,D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,a,b};l->ResourceBarrier(1,&x);
}
}
struct Renderer::Impl {
    ComPtr<ID3D12Device> device;
    ComPtr<ID3D12RootSignature> root;
    ComPtr<ID3D12PipelineState> pso;
    struct Slot {
        ComPtr<ID3D12DescriptorHeap> heap;
        ComPtr<ID3D12Resource> output[2],destination[2];
        std::shared_ptr<const framegen::Inputs> inputs[2];
        ComPtr<ID3D12Fence> fence;uint64_t value{};
        bool Idle()const{return !fence || fence->GetCompletedValue()>=value;}
    };
    std::array<Slot,3> slots;
    bool Init(ID3D12Device* d) {
        if(pso)return device.Get()==d;
        device=d;
        D3D12_DESCRIPTOR_RANGE ranges[2]{};
        ranges[0]={D3D12_DESCRIPTOR_RANGE_TYPE_SRV,1,0,0,0};
        ranges[1]={D3D12_DESCRIPTOR_RANGE_TYPE_UAV,1,0,0,1};
        D3D12_ROOT_PARAMETER params[2]{};
        params[0].ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;params[0].DescriptorTable={2,ranges};
        params[1].ParameterType=D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;params[1].Constants={0,0,8};
        D3D12_ROOT_SIGNATURE_DESC desc{};desc.NumParameters=2;desc.pParameters=params;
        ComPtr<ID3DBlob> blob,error,code;
        if(FAILED(D3D12SerializeRootSignature(&desc,D3D_ROOT_SIGNATURE_VERSION_1,&blob,&error)) ||
            FAILED(d->CreateRootSignature(0,blob->GetBufferPointer(),blob->GetBufferSize(),IID_PPV_ARGS(&root))) ||
            FAILED(D3DCompile(shader,sizeof(shader)-1,nullptr,nullptr,nullptr,"main","cs_5_0",D3DCOMPILE_ENABLE_STRICTNESS,0,&code,&error)))return false;
        D3D12_COMPUTE_PIPELINE_STATE_DESC state{};state.pRootSignature=root.Get();state.CS={code->GetBufferPointer(),code->GetBufferSize()};
        return SUCCEEDED(d->CreateComputePipelineState(&state,IID_PPV_ARGS(&pso)));
    }
};
Renderer::Renderer():impl(std::make_unique<Impl>()){}
Renderer::~Renderer()=default;
bool Renderer::Idle()const{for(const auto& s:impl->slots)if(!s.Idle())return false;return true;}
void Renderer::Poll(){for(auto& s:impl->slots)if(s.Idle())for(unsigned e=0;e<2;++e){s.inputs[e].reset();s.destination[e].Reset();}}
bool Renderer::Record(ID3D12Device* device,ID3D12GraphicsCommandList* list,ID3D12CommandQueue* queue,
    const std::shared_ptr<const framegen::Inputs> (&inputs)[2],ID3D12Resource* const (&destinations)[2],
    const Plan& plan,unsigned index,ID3D12Fence* fence,uint64_t value,bool crossQueue) {
    if(!device || !list || !queue || !fence || !value || index>=impl->slots.size())return false;
    if(impl->device && impl->device.Get()!=device){if(!Idle())return false;impl=std::make_unique<Impl>();}
    auto& slot=impl->slots[index];if(!slot.Idle() || !impl->Init(device))return false;
    // Shared queue retains FIFO ordering. A private presentation queue may
    // wait only on an already-submitted snapshot receipt, never future game work.
    for(unsigned e=0;e<2;++e) {
        if(!inputs[e] || (!crossQueue && inputs[e]->queue!=queue) || !inputs[e]->submitted || !inputs[e]->depth ||
            !inputs[e]->readyFence || !inputs[e]->fenceValue || !destinations[e])return false;
        const auto a=inputs[e]->depth->GetDesc(),b=destinations[e]->GetDesc();const auto& v=plan.eyes[e];
        if(!v.width || !v.height || a.Width!=v.width || a.Height!=v.height || b.Width!=v.width || b.Height!=v.height ||
            a.Format!=DXGI_FORMAT_R32_FLOAT || (b.Format!=DXGI_FORMAT_D32_FLOAT && b.Format!=DXGI_FORMAT_R32_TYPELESS) ||
            a.DepthOrArraySize!=1 || b.DepthOrArraySize!=1 || a.SampleDesc.Count!=1 || b.SampleDesc.Count!=1)return false;
    }
    for(unsigned e=0;e<2;++e)if(inputs[e]->queue!=queue &&
        FAILED(queue->Wait(inputs[e]->readyFence.Get(),inputs[e]->fenceValue)))return false;
    if(!slot.heap) {
        D3D12_DESCRIPTOR_HEAP_DESC desc{};desc.NumDescriptors=4;desc.Type=D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;desc.Flags=D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
        if(FAILED(device->CreateDescriptorHeap(&desc,IID_PPV_ARGS(&slot.heap))))return false;
    }
    const auto step=device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    for(unsigned e=0;e<2;++e) {
        const auto& v=plan.eyes[e];
        if(!slot.output[e] || slot.output[e]->GetDesc().Width!=v.width || slot.output[e]->GetDesc().Height!=v.height) {
            D3D12_RESOURCE_DESC d{};d.Dimension=D3D12_RESOURCE_DIMENSION_TEXTURE2D;d.Width=v.width;d.Height=v.height;
            d.DepthOrArraySize=1;d.MipLevels=1;d.Format=DXGI_FORMAT_R32_FLOAT;d.SampleDesc.Count=1;d.Flags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
            D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_DEFAULT;
            if(FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&d,Copy,nullptr,IID_PPV_ARGS(&slot.output[e]))))return false;
            slot.output[e]->SetName(L"CVR_OpenXR_UnjitteredDepth");
        }
        auto cpu=slot.heap->GetCPUDescriptorHandleForHeapStart();cpu.ptr+=2*e*step;
        D3D12_SHADER_RESOURCE_VIEW_DESC srv{};srv.Format=DXGI_FORMAT_R32_FLOAT;srv.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2D;
        srv.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;srv.Texture2D.MipLevels=1;
        device->CreateShaderResourceView(inputs[e]->depth.Get(),&srv,cpu);cpu.ptr+=step;
        D3D12_UNORDERED_ACCESS_VIEW_DESC uav{};uav.Format=DXGI_FORMAT_R32_FLOAT;uav.ViewDimension=D3D12_UAV_DIMENSION_TEXTURE2D;
        device->CreateUnorderedAccessView(slot.output[e].Get(),nullptr,&uav,cpu);
    }
    for(unsigned e=0;e<2;++e){slot.inputs[e]=inputs[e];slot.destination[e]=destinations[e];}
    slot.fence=fence;slot.value=value;
    ID3D12DescriptorHeap* heaps[]={slot.heap.Get()};list->SetDescriptorHeaps(1,heaps);
    list->SetComputeRootSignature(impl->root.Get());list->SetPipelineState(impl->pso.Get());
    for(unsigned e=0;e<2;++e) {
        const auto& v=plan.eyes[e];const auto& c=inputs[e]->camera;
        struct Constants {uint32_t w,h;float jitter[2],nearPlane,farPlane;uint32_t inverted,pad;} constants{v.width,v.height,{v.jitter[0],v.jitter[1]},c.nearPlane,c.farPlane,c.inverted?1u:0u,0};
        auto gpu=slot.heap->GetGPUDescriptorHandleForHeapStart();gpu.ptr+=2*e*step;
        Barrier(list,slot.output[e].Get(),Copy,Uav);
        list->SetComputeRootDescriptorTable(0,gpu);list->SetComputeRoot32BitConstants(1,8,&constants,0);
        list->Dispatch((v.width+7)/8,(v.height+7)/8,1);
        Barrier(list,slot.output[e].Get(),Uav,Copy);
        Barrier(list,destinations[e],D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_STATE_COPY_DEST);
        D3D12_TEXTURE_COPY_LOCATION from{},to{};from.pResource=slot.output[e].Get();to.pResource=destinations[e];
        from.Type=to.Type=D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        list->CopyTextureRegion(&to,0,0,0,&from,nullptr);
        Barrier(list,destinations[e],D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_DEPTH_WRITE);
    }
    return true;
}
}
