#include "Render/DepthCommitGate.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/StereoSceneState.hpp"
#include <d3dcompiler.h>
#include <intrin.h>
#include <cstring>
#include <new>

#if CVR_RUST_VRIK
// The gate is rust/crates/cvr-gpu/src/depth_commit_gate.rs; what stays here is StereoSceneState's snapshot and restore
// of the list's bindings around its compute work, attached to it.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's table is rust/crates/cvr-gpu/src/depth_commit_gate.rs' own
namespace {
static_assert(std::is_trivially_destructible_v<cvr::stereo::scene_state::Bindings> && alignof(cvr::stereo::scene_state::Bindings)<=8);
struct DepthGateAttach {size_t bindingsBytes;bool(*snapshot)(void*,void*);void(*restore)(const void*,void*);};
const DepthGateAttach depthGateTable{sizeof(cvr::stereo::scene_state::Bindings),
    [](void* list,void* storage){return cvr::stereo::scene_state::Snapshot(static_cast<ID3D12GraphicsCommandList*>(list),
        *new(storage) cvr::stereo::scene_state::Bindings{});},
    [](const void* storage,void* list){static_cast<const cvr::stereo::scene_state::Bindings*>(storage)->Restore(static_cast<ID3D12GraphicsCommandList*>(list));}};
extern "C" void cvr_depth_gate_attach(const DepthGateAttach*);
const bool depthGateAttached=(cvr_depth_gate_attach(&depthGateTable),true);
}
#endif
#else
namespace cvr::stereo {
using Microsoft::WRL::ComPtr;
struct DepthCommitGate::Storage final:IUnknown {
    std::atomic<ULONG> refs{1};ComPtr<ID3D12Device> device;
    ComPtr<ID3D12Resource> initial,upload,decision,readback;
    ComPtr<ID3D12RootSignature> root;ComPtr<ID3D12PipelineState> pipeline;ComPtr<ID3D12DescriptorHeap> heap;
    ComPtr<ID3D12RootSignature> bufferRoot;ComPtr<ID3D12PipelineState> bufferPipeline;
    void* mapped{};UINT width{},height{};bool prepared{},frozen{},copied{},reused{};
    ~Storage(){if(mapped){const D3D12_RANGE written{0,8};upload->Unmap(0,&written);}}
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n)delete this;return n;}
};
DepthCommitGate::DepthCommitGate()=default;
DepthCommitGate::~DepthCommitGate()=default;
HRESULT DepthCommitGate::Create(ID3D12Device* device,ID3D12Resource* initial)try{
    if(storage_ || !device || !initial)return E_INVALIDARG;
    const auto d=initial->GetDesc();ComPtr<ID3D12Device> actual;
    if(FAILED(initial->GetDevice(IID_PPV_ARGS(&actual))) || actual.Get()!=device || d.Dimension!=D3D12_RESOURCE_DIMENSION_TEXTURE2D ||
        d.DepthOrArraySize!=2 || d.MipLevels!=1 || !d.Width || d.Width>4096 || !d.Height || d.Height>4096 ||
        d.Format!=DXGI_FORMAT_R32G8X24_TYPELESS || d.SampleDesc.Count!=1 || d.SampleDesc.Quality || (d.Flags&D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE))return E_INVALIDARG;
    ComPtr<Storage> s;s.Attach(new Storage);s->device=device;s->initial=initial;s->width=UINT(d.Width);s->height=d.Height;
    static constexpr char shader[]=R"(
Texture2DArray<float> Depth:register(t0);
Texture2DArray<uint2> Stencil:register(t1);
RWByteAddressBuffer Decision:register(u0);
[numthreads(16,16,1)] void CS(uint3 id:SV_DispatchThreadID) {
    uint w,h,n;Depth.GetDimensions(w,h,n);if(id.x>=w || id.y>=h)return;
    int4 a=int4(id.xy,0,0),b=int4(id.xy,1,0);
    if(asuint(Depth.Load(a))!=asuint(Depth.Load(b)) || Stencil.Load(a).y!=Stencil.Load(b).y){
        uint ignored;Decision.InterlockedExchange(0,0,ignored);
    }
})";
    probe::Scope internal;ComPtr<ID3DBlob> code,error,signature;
    auto hr=D3DCompile(shader,std::strlen(shader),nullptr,nullptr,nullptr,"CS","cs_5_0",D3DCOMPILE_OPTIMIZATION_LEVEL3,0,&code,&error);if(FAILED(hr))return hr;
    D3D12_DESCRIPTOR_RANGE range{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,2,0,0,0};D3D12_ROOT_PARAMETER params[2]{};
    params[0].ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;params[0].DescriptorTable={1,&range};
    params[1].ParameterType=D3D12_ROOT_PARAMETER_TYPE_UAV;params[1].Descriptor={0,0};
    const D3D12_ROOT_SIGNATURE_DESC desc{2,params,0,nullptr,D3D12_ROOT_SIGNATURE_FLAG_NONE};
    hr=D3D12SerializeRootSignature(&desc,D3D_ROOT_SIGNATURE_VERSION_1,&signature,&error);if(FAILED(hr))return hr;
    hr=device->CreateRootSignature(0,signature->GetBufferPointer(),signature->GetBufferSize(),IID_PPV_ARGS(&s->root));if(FAILED(hr))return hr;
    D3D12_COMPUTE_PIPELINE_STATE_DESC pd{};pd.pRootSignature=s->root.Get();pd.CS={code->GetBufferPointer(),code->GetBufferSize()};
    hr=device->CreateComputePipelineState(&pd,IID_PPV_ARGS(&s->pipeline));if(FAILED(hr))return hr;
    static constexpr char bufferShader[]=R"(
ByteAddressBuffer Main:register(t0);
ByteAddressBuffer Peer:register(t1);
RWByteAddressBuffer Decision:register(u0);
cbuffer Size:register(b0){uint Words;uint RowBytes;uint RowPitch;uint Rows;};
[numthreads(64,1,1)] void CS(uint3 id:SV_DispatchThreadID) {
    if(id.x>=Words || id.y>=Rows)return;
    uint at=id.y*RowPitch+id.x*4,remaining=RowBytes-id.x*4;
    uint mask=remaining>=4?0xffffffff:((1u<<(remaining*8))-1);
    if(((Main.Load(at)^Peer.Load(at))&mask)!=0){
        uint ignored;Decision.InterlockedExchange(0,0,ignored);
    }
})";
    hr=D3DCompile(bufferShader,std::strlen(bufferShader),nullptr,nullptr,nullptr,"CS","cs_5_0",D3DCOMPILE_OPTIMIZATION_LEVEL3,0,&code,&error);if(FAILED(hr))return hr;
    D3D12_ROOT_PARAMETER bufferParams[4]{};
    for(UINT i=0;i<2;++i){bufferParams[i].ParameterType=D3D12_ROOT_PARAMETER_TYPE_SRV;bufferParams[i].Descriptor={i,0};}
    bufferParams[2].ParameterType=D3D12_ROOT_PARAMETER_TYPE_UAV;bufferParams[2].Descriptor={0,0};
    bufferParams[3].ParameterType=D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;bufferParams[3].Constants={0,0,4};
    const D3D12_ROOT_SIGNATURE_DESC bufferDesc{4,bufferParams,0,nullptr,D3D12_ROOT_SIGNATURE_FLAG_NONE};
    hr=D3D12SerializeRootSignature(&bufferDesc,D3D_ROOT_SIGNATURE_VERSION_1,&signature,&error);if(FAILED(hr))return hr;
    hr=device->CreateRootSignature(0,signature->GetBufferPointer(),signature->GetBufferSize(),IID_PPV_ARGS(&s->bufferRoot));if(FAILED(hr))return hr;
    pd.pRootSignature=s->bufferRoot.Get();pd.CS={code->GetBufferPointer(),code->GetBufferSize()};
    hr=device->CreateComputePipelineState(&pd,IID_PPV_ARGS(&s->bufferPipeline));if(FAILED(hr))return hr;
    const D3D12_DESCRIPTOR_HEAP_DESC hd{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,2,D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,0};
    hr=device->CreateDescriptorHeap(&hd,IID_PPV_ARGS(&s->heap));if(FAILED(hr))return hr;
    auto handle=s->heap->GetCPUDescriptorHandleForHeapStart();const auto stride=device->GetDescriptorHandleIncrementSize(hd.Type);
    for(UINT plane=0;plane<2;++plane){D3D12_SHADER_RESOURCE_VIEW_DESC view{};
        view.Format=plane?DXGI_FORMAT_X32_TYPELESS_G8X24_UINT:DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS;
        view.ViewDimension=D3D12_SRV_DIMENSION_TEXTURE2DARRAY;view.Shader4ComponentMapping=D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
        view.Texture2DArray.MipLevels=1;view.Texture2DArray.ArraySize=2;view.Texture2DArray.PlaneSlice=plane;
        device->CreateShaderResourceView(initial,&view,handle);handle.ptr+=stride;
    }
    D3D12_RESOURCE_DESC buffer{};buffer.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;buffer.Width=8;buffer.Height=1;
    buffer.DepthOrArraySize=buffer.MipLevels=1;buffer.SampleDesc.Count=1;buffer.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES hp{};hp.Type=D3D12_HEAP_TYPE_UPLOAD;
    hr=device->CreateCommittedResource(&hp,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&s->upload));if(FAILED(hr))return hr;
    const D3D12_RANGE none{};hr=s->upload->Map(0,&none,&s->mapped);if(FAILED(hr))return hr;if(!s->mapped)return E_FAIL;std::memset(s->mapped,0,8);
    hp.Type=D3D12_HEAP_TYPE_READBACK;
    hr=device->CreateCommittedResource(&hp,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&s->readback));if(FAILED(hr))return hr;
    hp.Type=D3D12_HEAP_TYPE_DEFAULT;buffer.Flags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    hr=device->CreateCommittedResource(&hp,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&s->decision));if(FAILED(hr))return hr;
    storage_=std::move(s);return S_OK;
}catch(const std::bad_alloc&){return E_OUTOFMEMORY;}
bool DepthCommitGate::Prepare(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw,const BufferProof* instances){
    return Prepare(list,draw,instances?std::span(instances,1):std::span<const BufferProof>{});
}
bool DepthCommitGate::Prepare(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw,std::span<const BufferProof> proofs){
    if(!storage_ || !list || storage_->prepared || (draw.flags&64) || !draw.pipeline || list->GetType()!=D3D12_COMMAND_LIST_TYPE_DIRECT)return false;
    if(proofs.size()>MaxBufferProofs)return false;
    auto& s=*storage_.Get();ComPtr<ID3D12Device> device;scene_state::Bindings saved;
    if(FAILED(list->GetDevice(IID_PPV_ARGS(&device))) || device.Get()!=s.device.Get() || !scene_state::Snapshot(list,saved) ||
        !gpu::KeepCommandResources(list,{&s}))return false;
    UINT totalBytes{};
    for(const auto& proof:proofs){
        if(!proof.bytes || proof.bytes%4 || proof.bytes>8*1024*1024 || proof.bytes>32*1024*1024-totalBytes)return false;
        totalBytes+=proof.bytes;
        if(proof.rowBytes || proof.rowPitch || proof.rows){
            if(!proof.rowBytes || proof.rowBytes>proof.bytes || !proof.rows || proof.rows>65535 || proof.rowPitch%4 ||
                proof.rowPitch<(uint64_t(proof.rowBytes)+3)/4*4 ||
                uint64_t(proof.rows-1)*proof.rowPitch+(uint64_t(proof.rowBytes)+3)/4*4>proof.bytes)return false;
        }
        if(proof.mainOffset%16 || proof.peerOffset%16)return false;
        const std::array sources{proof.main,proof.peer};const std::array offsets{proof.mainOffset,proof.peerOffset};
        for(UINT i=0;i<2;++i){auto* source=sources[i];
            ComPtr<ID3D12Device> actual;if(!source || FAILED(source->GetDevice(IID_PPV_ARGS(&actual))) || actual.Get()!=device.Get())return false;
            const auto desc=source->GetDesc();if(desc.Dimension!=D3D12_RESOURCE_DIMENSION_BUFFER || offsets[i]>desc.Width || proof.bytes>desc.Width-offsets[i] ||
                (desc.Flags&D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE) || !source->GetGPUVirtualAddress())return false;
        }
    }
    for(const auto& proof:proofs)if(!gpu::KeepCommandResources(list,{proof.main,proof.peer}))return false;
    probe::Scope internal;
    if(s.reused){D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        b.Transition={s.decision.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_PREDICATION,D3D12_RESOURCE_STATE_COPY_DEST};list->ResourceBarrier(1,&b);}
    list->CopyBufferRegion(s.decision.Get(),0,s.upload.Get(),0,8);
    D3D12_RESOURCE_BARRIER barriers[2]{};for(auto& b:barriers)b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barriers[0].Transition={s.decision.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_UNORDERED_ACCESS};
    barriers[1].Transition={s.initial.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_DEPTH_WRITE,D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE};
    list->ResourceBarrier(2,barriers);list->SetPipelineState(s.pipeline.Get());list->SetComputeRootSignature(s.root.Get());
    auto* heap=s.heap.Get();list->SetDescriptorHeaps(1,&heap);list->SetComputeRootDescriptorTable(0,heap->GetGPUDescriptorHandleForHeapStart());
    list->SetComputeRootUnorderedAccessView(1,s.decision->GetGPUVirtualAddress());list->Dispatch((s.width+15)/16,(s.height+15)/16,1);
    for(const auto& proof:proofs){
        D3D12_RESOURCE_BARRIER uav{};uav.Type=D3D12_RESOURCE_BARRIER_TYPE_UAV;uav.UAV.pResource=s.decision.Get();list->ResourceBarrier(1,&uav);
        list->SetPipelineState(s.bufferPipeline.Get());list->SetComputeRootSignature(s.bufferRoot.Get());
        list->SetComputeRootShaderResourceView(0,proof.main->GetGPUVirtualAddress()+proof.mainOffset);
        list->SetComputeRootShaderResourceView(1,proof.peer->GetGPUVirtualAddress()+proof.peerOffset);
        list->SetComputeRootUnorderedAccessView(2,s.decision->GetGPUVirtualAddress());
        const auto rowBytes=proof.rowBytes?proof.rowBytes:proof.bytes;
        const UINT layout[]{(rowBytes+3)/4,rowBytes,proof.rowBytes?proof.rowPitch:proof.bytes,proof.rowBytes?proof.rows:1};
        list->SetComputeRoot32BitConstants(3,4,layout,0);list->Dispatch((layout[0]+63)/64,layout[3],1);
    }
    barriers[0].Transition.StateBefore=D3D12_RESOURCE_STATE_UNORDERED_ACCESS;barriers[0].Transition.StateAfter=D3D12_RESOURCE_STATE_PREDICATION;
    std::swap(barriers[1].Transition.StateBefore,barriers[1].Transition.StateAfter);list->ResourceBarrier(2,barriers);
    saved.Restore(list);list->SetPipelineState(reinterpret_cast<ID3D12PipelineState*>(draw.pipeline));s.prepared=true;return true;
}
bool DepthCommitGate::Decide(bool allow){
    if(!storage_ || storage_->frozen)return false;
    *static_cast<volatile uint64_t*>(storage_->mapped)=allow?1:0;_mm_sfence();return true;
}
void DepthCommitGate::Freeze(){if(storage_)storage_->frozen=true;}
bool DepthCommitGate::Bind(ID3D12GraphicsCommandList* list,bool optimized)const{
    if(!storage_ || !list || !storage_->prepared || !gpu::KeepCommandResources(list,{storage_.Get()}))return false;
    probe::Scope internal;
    list->SetPredication(storage_->decision.Get(),0,optimized?D3D12_PREDICATION_OP_EQUAL_ZERO:D3D12_PREDICATION_OP_NOT_EQUAL_ZERO);return true;
}
bool DepthCommitGate::Readback(ID3D12GraphicsCommandList* list){
    if(!storage_ || !list || !storage_->prepared || storage_->copied || !gpu::KeepCommandResources(list,{storage_.Get()}))return false;
    auto& s=*storage_.Get();probe::Scope internal;list->SetPredication(nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);
    D3D12_RESOURCE_BARRIER b{};b.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition={s.decision.Get(),D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,D3D12_RESOURCE_STATE_PREDICATION,D3D12_RESOURCE_STATE_COPY_SOURCE};list->ResourceBarrier(1,&b);
    list->CopyBufferRegion(s.readback.Get(),0,s.decision.Get(),0,8);std::swap(b.Transition.StateBefore,b.Transition.StateAfter);list->ResourceBarrier(1,&b);
    s.copied=true;return true;
}
HRESULT DepthCommitGate::Read(uint64_t& value)const{
    if(!storage_ || !storage_->copied)return E_UNEXPECTED;
    void* data{};const D3D12_RANGE range{0,8};const auto hr=storage_->readback->Map(0,&range,&data);if(FAILED(hr))return hr;
    const D3D12_RANGE none{};if(!data){storage_->readback->Unmap(0,&none);return E_FAIL;}
    std::memcpy(&value,data,8);storage_->readback->Unmap(0,&none);return value<=1?S_OK:E_FAIL;
}
bool DepthCommitGate::Restart(){
    if(!storage_ || !storage_->prepared || !storage_->frozen || !storage_->copied)return false;
    auto& s=*storage_.Get();s.prepared=s.frozen=s.copied=false;s.reused=true;return Decide(false);
}
}
#endif
