#pragma once
#include "Render/NativeStereoProbe.hpp"
#include <wrl/client.h>
namespace cvr::stereo::scene_state {
struct Bindings {
    struct Rate {
        D3D12_SHADING_RATE value=D3D12_SHADING_RATE_1X1;
        std::array<D3D12_SHADING_RATE_COMBINER,2> combiners{D3D12_SHADING_RATE_COMBINER_PASSTHROUGH,D3D12_SHADING_RATE_COMBINER_PASSTHROUGH};
        ID3D12Resource* image{};
        bool known{};
    } rate;
    ID3D12RootSignature* root{};
    std::array<D3D12_GPU_DESCRIPTOR_HANDLE,64> tables{};
    uint64_t tableMask{};
    ID3D12RootSignature* computeRoot{};
    std::array<D3D12_GPU_DESCRIPTOR_HANDLE,64> computeTables{};
    uint64_t computeTableMask{};
    std::array<ID3D12DescriptorHeap*,2> heaps{};
    UINT heapCount{},stencil{};
    std::array<float,4> blendFactor{1,1,1,1};
    D3D12_PRIMITIVE_TOPOLOGY topology{};
    bool resetSeen{},heapsSeen{};
    void Restore(ID3D12GraphicsCommandList*) const;
};
struct Targets {std::array<Microsoft::WRL::ComPtr<ID3D12Resource>,4> resources;};
using TargetInfo=pass_targets::View;
struct GeometryTargets {
    std::array<Microsoft::WRL::ComPtr<ID3D12Resource>,5> resources;
    std::array<TargetInfo,5> views{};
    std::array<D3D12_CPU_DESCRIPTOR_HANDLE,5> handles{};
    UINT colors{};
};
struct DrawState {
    Microsoft::WRL::ComPtr<ID3D12PipelineState> original;
    Microsoft::WRL::ComPtr<ID3D12Resource> rateImage;
    Bindings::Rate rate;
    D3D12_PRIMITIVE_TOPOLOGY topology{};
    UINT stencil{};
    std::array<float,4> blendFactor{1,1,1,1};
    Targets targets;
};
#if CVR_RUST_VRIK
}
#include "Render/RenderProbeScope.hpp"
#include <algorithm>
#include <cstddef>
// The state: rust/crates/cvr-gpu/src/scene_state.rs. Its filters stay here: binding edits only while the probe
// records and not for the port's own commands, view creation only while bindings are observed or the probe is on.
namespace cvr::stereo::scene_state::detail {
struct DrawTargets {uint32_t rtCount,rtContiguous;uint64_t targets[8];uint64_t depth;D3D12_VIEWPORT viewport;};
struct DrawStateRaw {Bindings::Rate rate;ID3D12Resource* rateImage;D3D12_PRIMITIVE_TOPOLOGY topology;UINT stencil;float blendFactor[4];};
struct GeometryTargetsRaw {ID3D12Resource* resources[5];TargetInfo views[5];D3D12_CPU_DESCRIPTOR_HANDLE handles[5];UINT colors;};
static_assert(sizeof(Bindings::Rate)==32 && sizeof(Bindings)==1136 && offsetof(Bindings,tables)==40 &&
    offsetof(Bindings,computeTables)==568 && offsetof(Bindings,heaps)==1088 && offsetof(Bindings,topology)==1128);
static_assert(sizeof(DrawTargets)==104 && sizeof(DrawStateRaw)==64 && sizeof(GeometryTargetsRaw)==328);
inline DrawTargets Draw(const native_probe::DrawRecord& d){
    DrawTargets t{d.rtCount,d.rtContiguous,{},d.depth,d.viewport};std::copy(d.targets.begin(),d.targets.end(),t.targets);return t;
}
inline bool Recording(){return !probe::internalCommands && CyberpunkVR_NativeStereoProbeState.load(std::memory_order_relaxed)==1;}
inline bool ObservingViews(){return native_probe::ObserveBindings() || native_probe::Enabled();}
}
extern "C" {
void cvr_scene_root_created(ID3D12RootSignature*,const void*,size_t);
void cvr_scene_pipeline_created(ID3D12PipelineState*,bool);
bool cvr_scene_compatible(ID3D12PipelineState*);
void cvr_scene_reset_capture();
void cvr_scene_reset(ID3D12GraphicsCommandList*,bool);
void cvr_scene_root(ID3D12GraphicsCommandList*,ID3D12RootSignature*);
void cvr_scene_table(ID3D12GraphicsCommandList*,UINT,UINT64);
void cvr_scene_compute_root(ID3D12GraphicsCommandList*,ID3D12RootSignature*);
void cvr_scene_compute_table(ID3D12GraphicsCommandList*,UINT,UINT64);
void cvr_scene_heaps(ID3D12GraphicsCommandList*,UINT,ID3D12DescriptorHeap* const*);
void cvr_scene_topology(ID3D12GraphicsCommandList*,D3D12_PRIMITIVE_TOPOLOGY);
void cvr_scene_stencil(ID3D12GraphicsCommandList*,UINT);
void cvr_scene_blend_factor(ID3D12GraphicsCommandList*,const float*);
void cvr_scene_shading_rate(ID3D12GraphicsCommandList*,D3D12_SHADING_RATE,const D3D12_SHADING_RATE_COMBINER*);
void cvr_scene_shading_rate_image(ID3D12GraphicsCommandList*,ID3D12Resource*);
bool cvr_scene_snapshot(ID3D12GraphicsCommandList*,cvr::stereo::scene_state::Bindings*);
bool cvr_scene_capture_source_bindings(ID3D12GraphicsCommandList*,cvr::stereo::scene_state::Bindings*);
bool cvr_scene_capture_draw_state(ID3D12GraphicsCommandList*,cvr::stereo::scene_state::detail::DrawStateRaw*);
void cvr_scene_bindings_restore(const cvr::stereo::scene_state::Bindings*,ID3D12GraphicsCommandList*);
void cvr_scene_rtv_created(ID3D12Resource*,const D3D12_RENDER_TARGET_VIEW_DESC*,SIZE_T);
void cvr_scene_dsv_created(ID3D12Resource*,const D3D12_DEPTH_STENCIL_VIEW_DESC*,SIZE_T);
bool cvr_scene_capture_bound_resources(UINT,const D3D12_CPU_DESCRIPTOR_HANDLE*,bool,const D3D12_CPU_DESCRIPTOR_HANDLE*,ID3D12Resource* (*)[9]);
bool cvr_scene_capture_targets(const cvr::stereo::scene_state::detail::DrawTargets*,ID3D12Resource* (*)[4]);
bool cvr_scene_capture_geometry_targets(const cvr::stereo::scene_state::detail::DrawTargets*,cvr::stereo::scene_state::detail::GeometryTargetsRaw*);
bool cvr_scene_describe_targets(const cvr::stereo::scene_state::detail::DrawTargets*,cvr::stereo::pass_targets::View (*)[9]);
}
namespace cvr::stereo::scene_state {
inline void Bindings::Restore(ID3D12GraphicsCommandList* list) const{cvr_scene_bindings_restore(this,list);}
inline void RootCreated(ID3D12RootSignature* root,const void* data,size_t bytes){cvr_scene_root_created(root,data,bytes);}
inline void PipelineCreated(ID3D12PipelineState* pso,bool compatible){cvr_scene_pipeline_created(pso,compatible);}
inline bool Compatible(ID3D12PipelineState* pso){return cvr_scene_compatible(pso);}
inline void ResetCapture(){cvr_scene_reset_capture();}
inline void Reset(ID3D12GraphicsCommandList* list,bool rateKnown=false){if(detail::Recording())cvr_scene_reset(list,rateKnown);}
inline void Root(ID3D12GraphicsCommandList* list,ID3D12RootSignature* root){if(detail::Recording())cvr_scene_root(list,root);}
inline void Table(ID3D12GraphicsCommandList* list,UINT index,D3D12_GPU_DESCRIPTOR_HANDLE h){if(detail::Recording())cvr_scene_table(list,index,h.ptr);}
inline void ComputeRoot(ID3D12GraphicsCommandList* list,ID3D12RootSignature* root){if(detail::Recording())cvr_scene_compute_root(list,root);}
inline void ComputeTable(ID3D12GraphicsCommandList* list,UINT index,D3D12_GPU_DESCRIPTOR_HANDLE h){if(detail::Recording())cvr_scene_compute_table(list,index,h.ptr);}
inline void Heaps(ID3D12GraphicsCommandList* list,UINT count,ID3D12DescriptorHeap* const* heaps){if(detail::Recording())cvr_scene_heaps(list,count,heaps);}
inline void Topology(ID3D12GraphicsCommandList* list,D3D12_PRIMITIVE_TOPOLOGY t){if(detail::Recording())cvr_scene_topology(list,t);}
inline void Stencil(ID3D12GraphicsCommandList* list,UINT s){if(detail::Recording())cvr_scene_stencil(list,s);}
inline void BlendFactor(ID3D12GraphicsCommandList* list,const float* factors){if(detail::Recording())cvr_scene_blend_factor(list,factors);}
inline void ShadingRate(ID3D12GraphicsCommandList* list,D3D12_SHADING_RATE rate,const D3D12_SHADING_RATE_COMBINER* combiners){
    if(detail::Recording())cvr_scene_shading_rate(list,rate,combiners);}
inline void ShadingRateImage(ID3D12GraphicsCommandList* list,ID3D12Resource* image){if(detail::Recording())cvr_scene_shading_rate_image(list,image);}
inline bool Snapshot(ID3D12GraphicsCommandList* list,Bindings& result){return cvr_scene_snapshot(list,&result);}
inline bool CaptureSourceBindings(ID3D12GraphicsCommandList* list,Bindings& result){return cvr_scene_capture_source_bindings(list,&result);}
inline bool CaptureDrawState(ID3D12GraphicsCommandList* list,ID3D12PipelineState* original,DrawState& result){
    if(!list || !original)return false;
    detail::DrawStateRaw raw{};if(!cvr_scene_capture_draw_state(list,&raw))return false;
    DrawState state;state.original=original;state.rateImage.Attach(raw.rateImage);state.rate=raw.rate;
    state.topology=raw.topology;state.stencil=raw.stencil;std::copy_n(raw.blendFactor,4,state.blendFactor.begin());
    result=std::move(state);return true;
}
inline void RtvCreated(ID3D12Resource* resource,const D3D12_RENDER_TARGET_VIEW_DESC* view,D3D12_CPU_DESCRIPTOR_HANDLE h){
    if(detail::ObservingViews())cvr_scene_rtv_created(resource,view,h.ptr);}
inline void DsvCreated(ID3D12Resource* resource,const D3D12_DEPTH_STENCIL_VIEW_DESC* view,D3D12_CPU_DESCRIPTOR_HANDLE h){
    if(detail::ObservingViews())cvr_scene_dsv_created(resource,view,h.ptr);}
inline bool CaptureTargets(const native_probe::DrawRecord& draw,Targets& result){
    const auto targets=detail::Draw(draw);ID3D12Resource* raw[4]{};
    if(!cvr_scene_capture_targets(&targets,&raw))return false;
    Targets pending;for(UINT i=0;i<4;++i)pending.resources[i].Attach(raw[i]);result=std::move(pending);return true;
}
inline bool CaptureGeometryTargets(const native_probe::DrawRecord& draw,GeometryTargets& result){
    result={};const auto targets=detail::Draw(draw);detail::GeometryTargetsRaw raw{};
    if(!cvr_scene_capture_geometry_targets(&targets,&raw))return false;
    for(UINT i=0;i<5;++i){result.resources[i].Attach(raw.resources[i]);result.views[i]=raw.views[i];result.handles[i]=raw.handles[i];}
    result.colors=raw.colors;return true;
}
inline bool CaptureBoundResources(UINT count,const D3D12_CPU_DESCRIPTOR_HANDLE* handles,BOOL contiguous,const D3D12_CPU_DESCRIPTOR_HANDLE* depth,
    std::array<Microsoft::WRL::ComPtr<ID3D12Resource>,9>& output){
    output={};ID3D12Resource* raw[9]{};
    if(!cvr_scene_capture_bound_resources(count,handles,contiguous!=FALSE,depth,&raw))return false;
    for(UINT i=0;i<9;++i)output[i].Attach(raw[i]);return true;
}
inline bool DescribeTargets(const native_probe::DrawRecord& draw,std::array<TargetInfo,9>& result){
    const auto targets=detail::Draw(draw);TargetInfo raw[9]{};
    const bool described=cvr_scene_describe_targets(&targets,&raw);std::copy_n(raw,9,result.begin());return described;
}
}
#else
void RootCreated(ID3D12RootSignature*,const void*,size_t);
void PipelineCreated(ID3D12PipelineState*,bool resolveCompatible);
void ResetCapture();
void Reset(ID3D12GraphicsCommandList*,bool rateKnown=false);
void Root(ID3D12GraphicsCommandList*,ID3D12RootSignature*);
void Table(ID3D12GraphicsCommandList*,UINT,D3D12_GPU_DESCRIPTOR_HANDLE);
void ComputeRoot(ID3D12GraphicsCommandList*,ID3D12RootSignature*);
void ComputeTable(ID3D12GraphicsCommandList*,UINT,D3D12_GPU_DESCRIPTOR_HANDLE);
void Heaps(ID3D12GraphicsCommandList*,UINT,ID3D12DescriptorHeap* const*);
void Topology(ID3D12GraphicsCommandList*,D3D12_PRIMITIVE_TOPOLOGY);
void Stencil(ID3D12GraphicsCommandList*,UINT);
void BlendFactor(ID3D12GraphicsCommandList*,const float*);
void ShadingRate(ID3D12GraphicsCommandList*,D3D12_SHADING_RATE,const D3D12_SHADING_RATE_COMBINER*);
void ShadingRateImage(ID3D12GraphicsCommandList*,ID3D12Resource*);
bool Snapshot(ID3D12GraphicsCommandList*,Bindings&);
// Actual source-command bindings, including legal stencil255. Captures all
// initialized64-slot tables; shader-required coverage and descriptor contents
// remain separate caller receipts.
bool CaptureSourceBindings(ID3D12GraphicsCommandList*,Bindings&);
// Only at an actual native draw, while its PSO and bound image are live.
// Does not certify shader-specific dynamic depth/sample state or resource contents.
bool CaptureDrawState(ID3D12GraphicsCommandList*,ID3D12PipelineState*,DrawState&);
void RtvCreated(ID3D12Resource*,const D3D12_RENDER_TARGET_VIEW_DESC*,D3D12_CPU_DESCRIPTOR_HANDLE);
void DsvCreated(ID3D12Resource*,const D3D12_DEPTH_STENCIL_VIEW_DESC*,D3D12_CPU_DESCRIPTOR_HANDLE);
bool CaptureTargets(const native_probe::DrawRecord&,Targets&);
// Only at an actual native draw while the engine owns all bound resources.
// Slots 0..3 are colors; slot 4 is depth. Preserves read-only DSV flags.
// Identity and shape checks do not establish resource state or alias lifetime.
bool CaptureGeometryTargets(const native_probe::DrawRecord&,GeometryTargets&);
// At an actual OM/clear API call while its caller owns the bound resources.
// Resource-only lookup, including array/mip views and internal private targets;
// grants no whole-view, resource-state, residency or alias permission.
bool CaptureBoundResources(UINT,const D3D12_CPU_DESCRIPTOR_HANDLE*,BOOL,const D3D12_CPU_DESCRIPTOR_HANDLE*,
    std::array<Microsoft::WRL::ComPtr<ID3D12Resource>,9>&);
// Weak metadata captured at the actual draw. Identity is tied to COM resource
// lifetime; this does not retain a resource or prove its contents/alias state.
bool DescribeTargets(const native_probe::DrawRecord&,std::array<TargetInfo,9>&);
bool Compatible(ID3D12PipelineState*);
}
#endif
