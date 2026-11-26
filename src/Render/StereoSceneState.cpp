#if !CVR_RUST_VRIK
// The C++ build's scene state; the Rust build's is rust/crates/cvr-gpu/src/scene_state.rs (the header calls it).
#include "Render/StereoSceneState.hpp"
#include "Render/RenderProbeScope.hpp"
#include <mutex>
#include <unordered_map>
#include <algorithm>
namespace cvr::stereo::scene_state {
namespace {
std::mutex mutex;
std::unordered_map<ID3D12RootSignature*,uint64_t> roots;
std::unordered_map<ID3D12PipelineState*,bool> pipelines;
std::unordered_map<ID3D12GraphicsCommandList*,Bindings> bindings;
struct View {ID3D12Resource* resource{};bool depth{};D3D12_RESOURCE_DESC desc{};uint32_t format{},flags{},stride{};uint64_t identity{};bool whole=true;};
constexpr GUID targetIdentityKey{0x52fbc840,0x27d7,0x46ce,{0x82,0xe5,0x58,0xc5,0x1c,0xe5,0x8f,0x39}};
uint64_t nextTargetIdentity{};
// Weak metadata only. Retention happens at an actual draw where the engine owns
// every bound target. Rewrites invalidate previous entries, including null views.
std::unordered_map<SIZE_T,View> targets;
template<class F> void Edit(ID3D12GraphicsCommandList* list,F f) {
    if(probe::internalCommands || CyberpunkVR_NativeStereoProbeState.load(std::memory_order_relaxed)!=1)return;
    std::lock_guard lock(mutex);auto it=bindings.find(list);
    if(it!=bindings.end())f(it->second);else if(bindings.size()<512)f(bindings[list]);
}
bool Shape(const D3D12_RESOURCE_DESC& d) {return d.Dimension==D3D12_RESOURCE_DIMENSION_TEXTURE2D &&
    d.DepthOrArraySize==1 && d.MipLevels==1 && d.SampleDesc.Count==1 && d.Width && d.Height;}
void Store(D3D12_CPU_DESCRIPTOR_HANDLE h,View view) {
    std::lock_guard lock(mutex);if(!view.resource){targets.erase(h.ptr);return;}
    UINT bytes=sizeof(view.identity);
    if(FAILED(view.resource->GetPrivateData(targetIdentityKey,&bytes,&view.identity)) || bytes!=sizeof(view.identity) || !view.identity){
        view.identity=++nextTargetIdentity;
        if(FAILED(view.resource->SetPrivateData(targetIdentityKey,sizeof(view.identity),&view.identity))){targets.erase(h.ptr);return;}
    }
    if(targets.size()>=32768 && !targets.contains(h.ptr))targets.clear();targets[h.ptr]=view;
}
}
void RootCreated(ID3D12RootSignature* root,const void* data,size_t bytes) {
    Microsoft::WRL::ComPtr<ID3D12VersionedRootSignatureDeserializer> d;
    if(FAILED(D3D12CreateVersionedRootSignatureDeserializer(data,bytes,IID_PPV_ARGS(&d))))return;
    const D3D12_VERSIONED_ROOT_SIGNATURE_DESC* rs{};if(FAILED(d->GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1,&rs)))return;
    const auto n=rs->Desc_1_1.NumParameters;bool valid=n>0 && n<=64;
    for(UINT i=0;valid && i<n;++i)valid=rs->Desc_1_1.pParameters[i].ParameterType==D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
    std::lock_guard lock(mutex);if(roots.size()<128 || roots.contains(root))roots[root]=valid?(n==64?UINT64_MAX:(uint64_t(1)<<n)-1):0;
}
void PipelineCreated(ID3D12PipelineState* pso,bool compatible){std::lock_guard lock(mutex);if(pipelines.size()<8)pipelines[pso]=compatible;}
bool Compatible(ID3D12PipelineState* pso){std::lock_guard lock(mutex);const auto it=pipelines.find(pso);return it!=pipelines.end() && it->second;}
void ResetCapture(){std::lock_guard lock(mutex);bindings.clear();}
void Reset(ID3D12GraphicsCommandList* list,bool rateKnown){Edit(list,[&](auto& b){b={};b.resetSeen=true;b.rate.known=rateKnown;});}
void Root(ID3D12GraphicsCommandList* list,ID3D12RootSignature* root){Edit(list,[&](auto& b){if(b.root!=root){b.root=root;b.tableMask=0;b.tables={};}});}
void Table(ID3D12GraphicsCommandList* list,UINT index,D3D12_GPU_DESCRIPTOR_HANDLE h){if(index<64)Edit(list,[&](auto& b){b.tables[index]=h;b.tableMask|=uint64_t(1)<<index;});}
void ComputeRoot(ID3D12GraphicsCommandList* list,ID3D12RootSignature* root){Edit(list,[&](auto& b){if(b.computeRoot!=root){b.computeRoot=root;b.computeTableMask=0;b.computeTables={};}});}
void ComputeTable(ID3D12GraphicsCommandList* list,UINT index,D3D12_GPU_DESCRIPTOR_HANDLE h){if(index<64)Edit(list,[&](auto& b){b.computeTables[index]=h;b.computeTableMask|=uint64_t(1)<<index;});}
void Heaps(ID3D12GraphicsCommandList* list,UINT count,ID3D12DescriptorHeap* const* heaps){Edit(list,[&](auto& b){
    b.heapsSeen=count<=2 && (count==0 || heaps);if(!b.heapsSeen)return;
    bool changed=b.heapCount!=count;for(UINT i=0;i<count;++i)changed|=b.heaps[i]!=heaps[i];
    if(changed){b.tableMask=b.computeTableMask=0;b.tables={};b.computeTables={};}b.heapCount=count;b.heaps={};for(UINT i=0;i<count;++i)b.heaps[i]=heaps[i];});}
void Topology(ID3D12GraphicsCommandList* list,D3D12_PRIMITIVE_TOPOLOGY t){Edit(list,[&](auto& b){b.topology=t;});}
void Stencil(ID3D12GraphicsCommandList* list,UINT s){Edit(list,[&](auto& b){b.stencil=s&255;});}
void BlendFactor(ID3D12GraphicsCommandList* list,const float* factors){Edit(list,[&](auto& b){
    b.blendFactor={1,1,1,1};if(factors)std::copy_n(factors,4,b.blendFactor.begin());});}
void ShadingRate(ID3D12GraphicsCommandList* list,D3D12_SHADING_RATE rate,const D3D12_SHADING_RATE_COMBINER* combiners){Edit(list,[&](auto& b){b.rate.value=rate;
    b.rate.combiners=combiners?std::array{combiners[0],combiners[1]}:std::array{D3D12_SHADING_RATE_COMBINER_PASSTHROUGH,D3D12_SHADING_RATE_COMBINER_PASSTHROUGH};});}
void ShadingRateImage(ID3D12GraphicsCommandList* list,ID3D12Resource* image){Edit(list,[&](auto& b){b.rate.image=image;});}
bool Snapshot(ID3D12GraphicsCommandList* list,Bindings& result) {
    std::lock_guard lock(mutex);const auto it=bindings.find(list);if(it==bindings.end())return false;const auto& b=it->second;
    // Reset was observed, so a table never bound since then is known-unbound.
    // The native depth prepass leaves the unused pixel-stage tables unbound.
    // Restore every initialized table, including tables above slot 31, without
    // inventing descriptors for shader stages which the native PSO does not use.
    const auto root=roots.find(b.root);if(root==roots.end() || !root->second || (b.tableMask&~root->second) ||
        !b.resetSeen || !b.heapsSeen || !b.heapCount || !b.topology || b.stencil==255)return false;
    if(b.computeRoot){const auto compute=roots.find(b.computeRoot);if(compute==roots.end() || !compute->second)return false;}
    result=b;return true;
}
bool CaptureDrawState(ID3D12GraphicsCommandList* list,ID3D12PipelineState* original,DrawState& result) {
    if(!list || !original)return false;
    std::lock_guard lock(mutex);const auto it=bindings.find(list);if(it==bindings.end())return false;
    const auto& b=it->second;if(!b.resetSeen || !b.topology || !b.rate.known)return false;
    DrawState state;state.original=original;state.rateImage=b.rate.image;state.rate=b.rate;
    state.topology=b.topology;state.stencil=b.stencil;state.blendFactor=b.blendFactor;
    result=std::move(state);return true;
}
bool CaptureSourceBindings(ID3D12GraphicsCommandList* list,Bindings& result){
    std::lock_guard lock(mutex);const auto found=bindings.find(list);if(found==bindings.end())return false;
    const auto& value=found->second;const auto root=roots.find(value.root);
    if(root==roots.end() || !root->second || (value.tableMask&~root->second) || !value.resetSeen || !value.heapsSeen ||
        !value.heapCount || value.heapCount>2 || !value.topology || !value.rate.known)return false;
    if(value.computeRoot){const auto compute=roots.find(value.computeRoot);if(compute==roots.end() || !compute->second || (value.computeTableMask&~compute->second))return false;}
    for(UINT i=0;i<value.heapCount;++i)if(!value.heaps[i])return false;
    result=value;return true;
}
void Bindings::Restore(ID3D12GraphicsCommandList* list) const {
    list->SetDescriptorHeaps(heapCount,heaps.data());list->SetGraphicsRootSignature(root);
    for(UINT i=0;i<64;++i)if(tableMask&(uint64_t(1)<<i))list->SetGraphicsRootDescriptorTable(i,tables[i]);
    // Switching descriptor heaps invalidates compute tables as well. Preserve
    // those bindings even though this graphics resolve never dispatches compute.
    if(computeRoot){list->SetComputeRootSignature(computeRoot);for(UINT i=0;i<64;++i)if(computeTableMask&(uint64_t(1)<<i))list->SetComputeRootDescriptorTable(i,computeTables[i]);}
    list->IASetPrimitiveTopology(topology);list->OMSetStencilRef(stencil);list->OMSetBlendFactor(blendFactor.data());
}
void RtvCreated(ID3D12Resource* resource,const D3D12_RENDER_TARGET_VIEW_DESC* view,D3D12_CPU_DESCRIPTOR_HANDLE h) {
    if(!native_probe::ObserveBindings() && !native_probe::Enabled())return;View result{};
    if(resource){const auto d=resource->GetDesc();
        const bool whole=Shape(d) && (!view || (view->ViewDimension==D3D12_RTV_DIMENSION_TEXTURE2D && !view->Texture2D.MipSlice && !view->Texture2D.PlaneSlice));
        Microsoft::WRL::ComPtr<ID3D12Device> device;
        if(SUCCEEDED(resource->GetDevice(IID_PPV_ARGS(&device))))result={resource,false,d,UINT(view?view->Format:d.Format),0,
            device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV),0,whole};
    }Store(h,result);
}
void DsvCreated(ID3D12Resource* resource,const D3D12_DEPTH_STENCIL_VIEW_DESC* view,D3D12_CPU_DESCRIPTOR_HANDLE h) {
    if(!native_probe::ObserveBindings() && !native_probe::Enabled())return;View result{};
    if(resource){const auto d=resource->GetDesc();const bool whole=Shape(d) && (d.Format==19 || d.Format==20) &&
        ((!view && d.Format==20) || (view && view->Format==20 && view->ViewDimension==D3D12_DSV_DIMENSION_TEXTURE2D &&
            !(UINT(view->Flags)&~3u) && !view->Texture2D.MipSlice));
        result={resource,true,d,UINT(view?view->Format:d.Format),view?UINT(view->Flags):0,0,0,whole};}Store(h,result);
}
bool CaptureBoundResources(UINT count,const D3D12_CPU_DESCRIPTOR_HANDLE* handles,BOOL contiguous,const D3D12_CPU_DESCRIPTOR_HANDLE* depth,
    std::array<Microsoft::WRL::ComPtr<ID3D12Resource>,9>& output)try{
    output={};if(count>8 || (count && !handles) || (depth && !depth->ptr))return false;
    std::array<Microsoft::WRL::ComPtr<ID3D12Resource>,9> next;
    std::lock_guard lock(mutex);UINT stride{};
    if(count && contiguous){const auto first=targets.find(handles[0].ptr);
        if(first==targets.end() || first->second.depth || !first->second.stride)return false;stride=first->second.stride;}
    for(UINT i=0;i<9;++i){if(i<8 && i>=count)continue;if(i==8 && !depth)continue;
        const uint64_t offset=uint64_t(i)*stride;
        if(i<8 && contiguous && handles[0].ptr>SIZE_MAX-offset)return false;
        const auto handle=i==8?depth->ptr:contiguous?handles[0].ptr+offset:handles[i].ptr;
        if(!handle)return false;
        const auto found=targets.find(SIZE_T(handle));if(found==targets.end() || found->second.depth!=(i==8))return false;
        const auto& saved=found->second;if(!saved.resource || !saved.identity || (i<8 && contiguous && saved.stride!=stride))return false;
        const auto matches=[&](){uint64_t identity{};UINT bytes=sizeof(identity);
            if(FAILED(saved.resource->GetPrivateData(targetIdentityKey,&bytes,&identity)) || bytes!=sizeof(identity) || identity!=saved.identity)return false;
            const auto d=saved.resource->GetDesc();const auto& s=saved.desc;
            return d.Dimension==s.Dimension && d.Alignment==s.Alignment && d.Width==s.Width && d.Height==s.Height &&
                d.DepthOrArraySize==s.DepthOrArraySize && d.MipLevels==s.MipLevels && d.Format==s.Format && d.SampleDesc.Count==s.SampleDesc.Count &&
                d.SampleDesc.Quality==s.SampleDesc.Quality && d.Layout==s.Layout && d.Flags==s.Flags;};
        if(!matches())return false;next[i]=saved.resource;if(!matches())return false;
    }
    output=std::move(next);return true;
}catch(...){output={};return false;}
bool CaptureTargets(const native_probe::DrawRecord& draw,Targets& result) {
    if((draw.rtCount!=3 && draw.rtCount!=0) || (draw.rtCount && draw.rtContiguous) || !draw.depth || draw.viewport.TopLeftX || draw.viewport.TopLeftY ||
        draw.viewport.MinDepth!=0 || draw.viewport.MaxDepth!=1)return false;
    Targets pending;std::lock_guard lock(mutex);
    for(UINT i=draw.rtCount?0:3;i<4;++i){const auto it=targets.find(i<3?draw.targets[i]:draw.depth);if(it==targets.end() || !it->second.whole || it->second.depth!=(i==3) || it->second.flags)return false;
        auto* resource=it->second.resource;const auto d=resource->GetDesc();
        if(!Shape(d) || d.Width!=draw.viewport.Width || d.Height!=draw.viewport.Height || d.Width>2048 || d.Height>2048 ||
            (i<2 && (d.Format!=24 || it->second.format!=24)) || (i==2 && (d.Format!=28 || it->second.format!=28)) ||
            (i==3 && d.Format!=19 && d.Format!=20))return false;
        pending.resources[i]=resource;
    }result=std::move(pending);return true;
}
bool CaptureGeometryTargets(const native_probe::DrawRecord& draw,GeometryTargets& result)try{
    result={};
    if((draw.rtCount!=0 && draw.rtCount!=3 && draw.rtCount!=4) || !draw.depth ||
        draw.viewport.TopLeftX!=0 || draw.viewport.TopLeftY!=0 || draw.viewport.MinDepth!=0 || draw.viewport.MaxDepth!=1 ||
        !(draw.viewport.Width>=1 && draw.viewport.Width<=4096) || !(draw.viewport.Height>=1 && draw.viewport.Height<=4096))return false;
    const auto sameDesc=[](const D3D12_RESOURCE_DESC& a,const D3D12_RESOURCE_DESC& b){
        return a.Dimension==b.Dimension && a.Alignment==b.Alignment && a.Width==b.Width && a.Height==b.Height &&
            a.DepthOrArraySize==b.DepthOrArraySize && a.MipLevels==b.MipLevels && a.Format==b.Format &&
            a.SampleDesc.Count==b.SampleDesc.Count && a.SampleDesc.Quality==b.SampleDesc.Quality && a.Layout==b.Layout && a.Flags==b.Flags;
    };
    const auto cookie=[](ID3D12Resource* resource,uint64_t expected){
        uint64_t actual{};UINT bytes=sizeof(actual);
        return resource && expected && SUCCEEDED(resource->GetPrivateData(targetIdentityKey,&bytes,&actual)) &&
            bytes==sizeof(actual) && actual==expected;
    };
    GeometryTargets pending;pending.colors=draw.rtCount;
    std::array<View,5> saved{};Microsoft::WRL::ComPtr<IUnknown> deviceIdentity;
    std::lock_guard lock(mutex);uint32_t stride{};
    if(draw.rtCount && draw.rtContiguous){
        const auto first=targets.find(draw.targets[0]);
        if(first==targets.end() || first->second.depth || !first->second.stride)return false;
        stride=first->second.stride;
    }
    constexpr std::array<DXGI_FORMAT,4> formats{DXGI_FORMAT_R10G10B10A2_UNORM,DXGI_FORMAT_R10G10B10A2_UNORM,
        DXGI_FORMAT_R8G8B8A8_UNORM,DXGI_FORMAT_R16G16_FLOAT};
    for(UINT i=0;i<5;++i){
        if(i<4 && i>=draw.rtCount)continue;
        const uint64_t offset=uint64_t(i)*stride;
        if(i<4 && draw.rtContiguous && draw.targets[0]>UINT64_MAX-offset)return false;
        const auto handle=i==4?draw.depth:draw.rtContiguous?draw.targets[0]+offset:draw.targets[i];
        if(!handle)return false;
        const auto found=targets.find(handle);
        if(found==targets.end() || !found->second.whole || found->second.depth!=(i==4))return false;
        const auto view=found->second;auto* resource=view.resource;
        for(UINT previous=0;previous<i;++previous)if(pending.resources[previous].Get()==resource)return false;
        if(!cookie(resource,view.identity) || (i<4 && (view.flags || view.format!=UINT(formats[i]))) ||
            (i==4 && (view.format!=DXGI_FORMAT_D32_FLOAT_S8X24_UINT || (view.flags&~3u))) ||
            (i<4 && draw.rtContiguous && view.stride!=stride))return false;
        const auto desc=resource->GetDesc();
        if(!sameDesc(desc,view.desc) || !Shape(desc) || desc.SampleDesc.Quality || desc.Layout!=D3D12_TEXTURE_LAYOUT_UNKNOWN ||
            desc.Width!=draw.viewport.Width || desc.Height!=draw.viewport.Height ||
            (i<4 && (desc.Format!=formats[i] || !(desc.Flags&D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET))) ||
            (i==4 && ((desc.Format!=DXGI_FORMAT_R32G8X24_TYPELESS && desc.Format!=DXGI_FORMAT_D32_FLOAT_S8X24_UINT) ||
                !(desc.Flags&D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL))))return false;
        pending.resources[i]=resource;
        if(pending.resources[i].Get()!=view.resource || !cookie(pending.resources[i].Get(),view.identity) ||
            !sameDesc(pending.resources[i]->GetDesc(),view.desc))return false;
        Microsoft::WRL::ComPtr<ID3D12Device> device;Microsoft::WRL::ComPtr<IUnknown> identity;
        if(FAILED(resource->GetDevice(IID_PPV_ARGS(&device))) || !device || FAILED(device.As(&identity)) || !identity)return false;
        if(deviceIdentity && identity!=deviceIdentity)return false;
        if(!deviceIdentity)deviceIdentity=identity;
        saved[i]=view;pending.handles[i]={handle};
        pending.views[i]={view.identity,reinterpret_cast<uintptr_t>(resource),desc.Width,desc.Height,
            UINT(desc.Format),view.format,UINT(desc.Flags),view.flags,0};
    }
    for(UINT i=0;i<5;++i){
        if(!pending.resources[i])continue;
        const auto found=targets.find(pending.handles[i].ptr);const auto& view=saved[i];
        if(found==targets.end() || found->second.resource!=view.resource || found->second.identity!=view.identity ||
            found->second.depth!=view.depth || found->second.format!=view.format || found->second.flags!=view.flags ||
            found->second.stride!=view.stride || !sameDesc(found->second.desc,view.desc) ||
            !cookie(pending.resources[i].Get(),view.identity) || !sameDesc(pending.resources[i]->GetDesc(),view.desc))return false;
    }
    result=std::move(pending);return true;
}catch(...){result={};return false;}
bool DescribeTargets(const native_probe::DrawRecord& draw,std::array<TargetInfo,9>& result){
    result={};if(draw.rtCount>8 || (!draw.rtCount && !draw.depth))return false;
    std::lock_guard lock(mutex);std::array<TargetInfo,9> pending{};uint32_t stride{};
    if(draw.rtCount && draw.rtContiguous){const auto first=targets.find(draw.targets[0]);
        if(first==targets.end() || first->second.depth || !first->second.stride)return false;stride=first->second.stride;}
    for(UINT i=0;i<9;++i){if(i<8 && i>=draw.rtCount)continue;if(i==8 && !draw.depth)continue;
        const auto offset=uint64_t(i)*stride;if(i<8 && draw.rtContiguous && draw.targets[0]>UINT64_MAX-offset)return false;
        const auto handle=i==8?draw.depth:draw.rtContiguous?draw.targets[0]+offset:draw.targets[i];
        const auto found=targets.find(handle);if(found==targets.end() || !found->second.whole || found->second.depth!=(i==8))return false;
        const auto& v=found->second;pending[i]={v.identity,reinterpret_cast<uintptr_t>(v.resource),v.desc.Width,v.desc.Height,
            UINT(v.desc.Format),v.format,UINT(v.desc.Flags),v.flags,0};
    }
    result=pending;return true;
}
}
#endif
