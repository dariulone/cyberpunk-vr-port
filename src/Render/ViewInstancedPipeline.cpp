#include "Render/ViewInstancedPipeline.hpp"
#include <cstdio>
#include <cstring>
#include <bitset>
#include <utility>

namespace cvr::stereo {
namespace {
// D3D12's pipeline stream ABI requires pointer-aligned subobjects and padding.
#pragma warning(push)
#pragma warning(disable:4324)
template<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE Type,class T> struct alignas(void*) Part {
    D3D12_PIPELINE_STATE_SUBOBJECT_TYPE type=Type;
    T value{};
};
struct Stream {
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE,ID3D12RootSignature*> root;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS,D3D12_SHADER_BYTECODE> vs;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS,D3D12_SHADER_BYTECODE> ps;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DS,D3D12_SHADER_BYTECODE> ds;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_HS,D3D12_SHADER_BYTECODE> hs;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_GS,D3D12_SHADER_BYTECODE> gs;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_STREAM_OUTPUT,D3D12_STREAM_OUTPUT_DESC> streamOutput;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND,D3D12_BLEND_DESC> blend;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_MASK,UINT> sampleMask;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RASTERIZER,D3D12_RASTERIZER_DESC> raster;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL,D3D12_DEPTH_STENCIL_DESC> depth;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT,D3D12_INPUT_LAYOUT_DESC> input;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_IB_STRIP_CUT_VALUE,D3D12_INDEX_BUFFER_STRIP_CUT_VALUE> stripCut;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PRIMITIVE_TOPOLOGY,D3D12_PRIMITIVE_TOPOLOGY_TYPE> topology;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS,D3D12_RT_FORMAT_ARRAY> targets;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL_FORMAT,DXGI_FORMAT> depthFormat;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_DESC,DXGI_SAMPLE_DESC> samples;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_NODE_MASK,UINT> nodeMask;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_FLAGS,D3D12_PIPELINE_STATE_FLAGS> flags;
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VIEW_INSTANCING,D3D12_VIEW_INSTANCING_DESC> views;
};
#pragma warning(pop)
struct PartLayout {size_t payload{},bytes{};};
constexpr size_t Align(size_t n,size_t alignment){return (n+alignment-1)&~(alignment-1);}
template<class T> constexpr PartLayout Layout() {
    constexpr auto payload=Align(sizeof(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE),alignof(T));
    return {payload,Align(payload+sizeof(T),alignof(void*))};
}
PartLayout Layout(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE type) {
    switch(type) {
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE:return Layout<ID3D12RootSignature*>();
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS:case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS:
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DS:case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_HS:
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_GS:case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CS:
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS:case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS:return Layout<D3D12_SHADER_BYTECODE>();
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_STREAM_OUTPUT:return Layout<D3D12_STREAM_OUTPUT_DESC>();
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND:return Layout<D3D12_BLEND_DESC>();
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_MASK:case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_NODE_MASK:return Layout<UINT>();
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RASTERIZER:return Layout<D3D12_RASTERIZER_DESC>();
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL:return Layout<D3D12_DEPTH_STENCIL_DESC>();
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL1:return Layout<D3D12_DEPTH_STENCIL_DESC1>();
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT:return Layout<D3D12_INPUT_LAYOUT_DESC>();
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_IB_STRIP_CUT_VALUE:return Layout<D3D12_INDEX_BUFFER_STRIP_CUT_VALUE>();
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PRIMITIVE_TOPOLOGY:return Layout<D3D12_PRIMITIVE_TOPOLOGY_TYPE>();
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS:return Layout<D3D12_RT_FORMAT_ARRAY>();
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL_FORMAT:return Layout<DXGI_FORMAT>();
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_DESC:return Layout<DXGI_SAMPLE_DESC>();
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CACHED_PSO:return Layout<D3D12_CACHED_PIPELINE_STATE>();
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_FLAGS:return Layout<D3D12_PIPELINE_STATE_FLAGS>();
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VIEW_INSTANCING:return Layout<D3D12_VIEW_INSTANCING_DESC>();
    default:return {};
    }
}
template<class Visit> bool Walk(const D3D12_PIPELINE_STATE_STREAM_DESC& source,Visit visit) {
    if(!source.pPipelineStateSubobjectStream || !source.SizeInBytes || source.SizeInBytes>65536)return false;
    const auto* bytes=static_cast<const uint8_t*>(source.pPipelineStateSubobjectStream);
    size_t offset{};std::bitset<64> seen;
    while(offset<source.SizeInBytes) {
        if(source.SizeInBytes-offset<sizeof(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE))return false;
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE type{};std::memcpy(&type,bytes+offset,sizeof(type));
        const auto tag=static_cast<unsigned>(type);if(tag>=seen.size() || seen[tag])return false;seen.set(tag);
        const auto part=Layout(type);if(!part.bytes || part.bytes>source.SizeInBytes-offset)return false;
        if(!visit(type,offset+part.payload,bytes+offset+part.payload))return false;
        offset+=part.bytes;
    }
    return true;
}
}

bool ReadPipelineStream(const D3D12_PIPELINE_STATE_STREAM_DESC& source,PipelineStreamInfo& output) {
    PipelineStreamInfo result;
    const bool valid=Walk(source,[&](auto type,size_t,const uint8_t* value){
        switch(type) {
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS:std::memcpy(&result.vs,value,sizeof(result.vs));break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS:std::memcpy(&result.ps,value,sizeof(result.ps));break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_GS:
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_HS:
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DS:
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CS:
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS:
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS:{D3D12_SHADER_BYTECODE shader{};std::memcpy(&shader,value,sizeof(shader));result.otherStages|=shader.BytecodeLength!=0;break;}
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RASTERIZER:{D3D12_RASTERIZER_DESC raster{};std::memcpy(&raster,value,sizeof(raster));result.forcedSamples=raster.ForcedSampleCount;break;}
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VIEW_INSTANCING:result.viewInstancing=true;break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_STREAM_OUTPUT:{D3D12_STREAM_OUTPUT_DESC d{};std::memcpy(&d,value,sizeof(d));result.streamOutput=d.NumEntries || d.NumStrides;break;}
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE:std::memcpy(&result.root,value,sizeof(result.root));break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS:std::memcpy(&result.targets,value,sizeof(result.targets));break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL_FORMAT:std::memcpy(&result.depthFormat,value,sizeof(result.depthFormat));break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND:std::memcpy(&result.blend,value,sizeof(result.blend));result.haveBlend=true;break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_DESC:std::memcpy(&result.samples,value,sizeof(result.samples));break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PRIMITIVE_TOPOLOGY:std::memcpy(&result.topology,value,sizeof(result.topology));break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_FLAGS:std::memcpy(&result.flags,value,sizeof(result.flags));break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT:std::memcpy(&result.input,value,sizeof(result.input));break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL:std::memcpy(&result.depth,value,sizeof(result.depth));result.haveDepth=true;break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL1:{D3D12_DEPTH_STENCIL_DESC1 d{};std::memcpy(&d,value,sizeof(d));
            result.depth={d.DepthEnable,d.DepthWriteMask,d.DepthFunc,d.StencilEnable,d.StencilReadMask,d.StencilWriteMask,d.FrontFace,d.BackFace};result.haveDepth=true;result.depthBounds=d.DepthBoundsTestEnable!=FALSE;break;}
        default:break;
        }return true;
    });
    if(!valid || result.targets.NumRenderTargets>8)return false;output=result;return true;
}
bool EditPipelineStream(const D3D12_PIPELINE_STATE_STREAM_DESC& source,std::vector<uint8_t>& output,
                        const std::function<void(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE,uint8_t*)>& edit) {
    std::vector<uint8_t> copy(static_cast<const uint8_t*>(source.pPipelineStateSubobjectStream),
                              static_cast<const uint8_t*>(source.pPipelineStateSubobjectStream)+source.SizeInBytes);
    const bool ok=Walk(source,[&](auto type,size_t offset,const uint8_t*){edit(type,copy.data()+offset);return true;});
    if(ok)output=std::move(copy);
    return ok;
}
std::string DescribePipelineStream(const D3D12_PIPELINE_STATE_STREAM_DESC& source) {
    std::string out;char b[320];
    Walk(source,[&](auto type,size_t,const uint8_t* value){
        switch(type) {
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RASTERIZER:{D3D12_RASTERIZER_DESC r{};std::memcpy(&r,value,sizeof(r));
            sprintf_s(b," raster(fill %d cull %d ccw %d bias %d clip %d msaa %d aa %d forced %u conservative %d)",int(r.FillMode),int(r.CullMode),
                      r.FrontCounterClockwise,r.DepthBias,r.DepthClipEnable,r.MultisampleEnable,r.AntialiasedLineEnable,r.ForcedSampleCount,
                      int(r.ConservativeRaster));out+=b;break;}
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_BLEND:{D3D12_BLEND_DESC d{};std::memcpy(&d,value,sizeof(d));
            sprintf_s(b," blend(a2c %d independent %d rt0 %d logic %d)",d.AlphaToCoverageEnable,d.IndependentBlendEnable,d.RenderTarget[0].BlendEnable,
                      d.RenderTarget[0].LogicOpEnable);out+=b;break;}
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL:{D3D12_DEPTH_STENCIL_DESC d{};std::memcpy(&d,value,sizeof(d));
            sprintf_s(b," depth(%d write %d func %d stencil %d)",d.DepthEnable,int(d.DepthWriteMask),int(d.DepthFunc),d.StencilEnable);out+=b;break;}
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL1:{D3D12_DEPTH_STENCIL_DESC1 d{};std::memcpy(&d,value,sizeof(d));
            sprintf_s(b," depth1(%d write %d func %d stencil %d bounds %d)",d.DepthEnable,int(d.DepthWriteMask),int(d.DepthFunc),d.StencilEnable,
                      d.DepthBoundsTestEnable);out+=b;break;}
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS:{D3D12_RT_FORMAT_ARRAY a{};std::memcpy(&a,value,sizeof(a));
            out+=" rt(";for(UINT i=0;i<a.NumRenderTargets && i<8;++i){sprintf_s(b,"%s%d",i?",":"",int(a.RTFormats[i]));out+=b;}out+=")";break;}
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL_FORMAT:{DXGI_FORMAT f{};std::memcpy(&f,value,sizeof(f));sprintf_s(b," dsv %d",int(f));out+=b;break;}
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_DESC:{DXGI_SAMPLE_DESC s{};std::memcpy(&s,value,sizeof(s));sprintf_s(b," samples %u/%u",s.Count,s.Quality);out+=b;break;}
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_SAMPLE_MASK:{UINT m{};std::memcpy(&m,value,sizeof(m));sprintf_s(b," mask %08X",m);out+=b;break;}
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT:{D3D12_INPUT_LAYOUT_DESC d{};std::memcpy(&d,value,sizeof(d));
            sprintf_s(b," inputs %u",d.NumElements);out+=b;
            for(UINT i=0;i<d.NumElements && i<16 && d.pInputElementDescs;++i){const auto& e=d.pInputElementDescs[i];
                sprintf_s(b," [%s%u f%d s%u o%u c%d r%u]",e.SemanticName?e.SemanticName:"?",e.SemanticIndex,int(e.Format),e.InputSlot,
                          e.AlignedByteOffset,int(e.InputSlotClass),e.InstanceDataStepRate);out+=b;}
            break;}
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PRIMITIVE_TOPOLOGY:{D3D12_PRIMITIVE_TOPOLOGY_TYPE t{};std::memcpy(&t,value,sizeof(t));sprintf_s(b," topology %d",int(t));out+=b;break;}
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_IB_STRIP_CUT_VALUE:{D3D12_INDEX_BUFFER_STRIP_CUT_VALUE v{};std::memcpy(&v,value,sizeof(v));sprintf_s(b," cut %d",int(v));out+=b;break;}
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_FLAGS:{D3D12_PIPELINE_STATE_FLAGS f{};std::memcpy(&f,value,sizeof(f));sprintf_s(b," flags %X",unsigned(f));out+=b;break;}
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_NODE_MASK:{UINT m{};std::memcpy(&m,value,sizeof(m));sprintf_s(b," node %u",m);out+=b;break;}
        default:sprintf_s(b," #%d",int(type));out+=b;break;
        }
        return true;
    });
    return out;
}
bool FindPipelineStreamOffsets(const D3D12_PIPELINE_STATE_STREAM_DESC& source,PipelineStreamOffsets& output) {
    PipelineStreamOffsets result;
    const bool valid=Walk(source,[&](auto type,size_t offset,const uint8_t*) {
        switch(type) {
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS:result.vs=offset;break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS:result.ps=offset;break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CS:result.cs=offset;break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CACHED_PSO:result.cached=offset;break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS:result.ms=offset;break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS:result.as=offset;break;
        default:break;
        }return true;
    });
    if(valid)output=result;return valid;
}
bool BuildViewInstancedStream(const D3D12_PIPELINE_STATE_STREAM_DESC& source,const D3D12_SHADER_BYTECODE& vs,
    const ViewInstancedLayout& layout,std::vector<uint8_t>& output,const D3D12_SHADER_BYTECODE& ps,ID3D12RootSignature* root) {
    if(!vs.pShaderBytecode || !vs.BytecodeLength)return false;
    if(bool(ps.pShaderBytecode)!=bool(ps.BytecodeLength))return false;
    std::vector<uint8_t> copy;bool found{},foundPs{},foundRoot{};
    const bool valid=Walk(source,[&](auto type,size_t offset,const uint8_t* value){
        if(type==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VIEW_INSTANCING)return false;
        if(copy.empty())copy.assign(static_cast<const uint8_t*>(source.pPipelineStateSubobjectStream),
            static_cast<const uint8_t*>(source.pPipelineStateSubobjectStream)+source.SizeInBytes);
        if(type==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS){std::memcpy(copy.data()+offset,&vs,sizeof(vs));found=true;}
        if(type==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS){foundPs=true;if(ps.BytecodeLength)std::memcpy(copy.data()+offset,&ps,sizeof(ps));}
        if(type==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE){foundRoot=true;if(root)std::memcpy(copy.data()+offset,&root,sizeof(root));}
        if(type==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CACHED_PSO){D3D12_CACHED_PIPELINE_STATE empty{};std::memcpy(copy.data()+offset,&empty,sizeof(empty));}
        if(type==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CS || type==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS || type==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS){
            D3D12_SHADER_BYTECODE shader{};std::memcpy(&shader,value,sizeof(shader));if(shader.BytecodeLength)return false;
        }
        return true;
    });
    if(!valid || !found || (ps.BytecodeLength && !foundPs))return false;
    if(root && !foundRoot) {
        Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_ROOT_SIGNATURE,ID3D12RootSignature*> part;part.value=root;
        const auto* data=reinterpret_cast<const uint8_t*>(&part);copy.insert(copy.end(),data,data+sizeof(part));
    }
    Part<D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VIEW_INSTANCING,D3D12_VIEW_INSTANCING_DESC> views;
    views.value={2,layout.locations.data(),layout.allowMasking?D3D12_VIEW_INSTANCING_FLAG_ENABLE_VIEW_INSTANCE_MASKING:D3D12_VIEW_INSTANCING_FLAG_NONE};
    const auto* data=reinterpret_cast<const uint8_t*>(&views);copy.insert(copy.end(),data,data+sizeof(views));output=std::move(copy);return true;
}
HRESULT CreateViewInstancedPipelineStream(ID3D12Device2* device,const D3D12_PIPELINE_STATE_STREAM_DESC& source,
    const D3D12_SHADER_BYTECODE& vs,const ViewInstancedLayout& layout,ID3D12PipelineState** result,const D3D12_SHADER_BYTECODE& ps,ID3D12RootSignature* root) {
    if(!result)return E_POINTER;*result=nullptr;if(!device)return E_INVALIDARG;
    D3D12_FEATURE_DATA_D3D12_OPTIONS3 options{};
    const auto capability=device->CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS3,&options,sizeof(options));
    if(FAILED(capability))return capability;
    if(options.ViewInstancingTier==D3D12_VIEW_INSTANCING_TIER_NOT_SUPPORTED)return DXGI_ERROR_UNSUPPORTED;
    std::vector<uint8_t> copy;if(!BuildViewInstancedStream(source,vs,layout,copy,ps,root))return E_INVALIDARG;
    D3D12_PIPELINE_STATE_STREAM_DESC stream{copy.size(),copy.data()};return device->CreatePipelineState(&stream,IID_PPV_ARGS(result));
}

bool GraphicsDescStream(const D3D12_GRAPHICS_PIPELINE_STATE_DESC& source,std::vector<uint8_t>& output) {
    if(!source.VS.pShaderBytecode || !source.VS.BytecodeLength || source.NumRenderTargets>D3D12_SIMULTANEOUS_RENDER_TARGET_COUNT)
        return false;
    Stream s;
    s.root.value=source.pRootSignature;s.vs.value=source.VS;s.ps.value=source.PS;
    s.ds.value=source.DS;s.hs.value=source.HS;s.gs.value=source.GS;s.streamOutput.value=source.StreamOutput;
    s.blend.value=source.BlendState;s.sampleMask.value=source.SampleMask;
    s.raster.value=source.RasterizerState;s.depth.value=source.DepthStencilState;
    s.input.value=source.InputLayout;s.stripCut.value=source.IBStripCutValue;s.topology.value=source.PrimitiveTopologyType;
    s.targets.value.NumRenderTargets=source.NumRenderTargets;
    for(UINT i=0;i<source.NumRenderTargets;++i)s.targets.value.RTFormats[i]=source.RTVFormats[i];
    s.depthFormat.value=source.DSVFormat;s.samples.value=source.SampleDesc;
    s.nodeMask.value=source.NodeMask;s.flags.value=source.Flags;
    const auto* bytes=reinterpret_cast<const uint8_t*>(&s);
    output.assign(bytes,bytes+offsetof(Stream,views));   // no view instancing part: the builder adds its own
    return true;
}
HRESULT CreateViewInstancedPipeline(ID3D12Device2* device,
    const D3D12_GRAPHICS_PIPELINE_STATE_DESC& source,const ViewInstancedLayout& layout,
    ID3D12PipelineState** result) {
    if(!result)return E_POINTER;
    *result=nullptr;
    if(!device || !source.VS.pShaderBytecode || !source.VS.BytecodeLength ||
       source.NumRenderTargets>D3D12_SIMULTANEOUS_RENDER_TARGET_COUNT)return E_INVALIDARG;
    D3D12_FEATURE_DATA_D3D12_OPTIONS3 options{};
    const auto capability=device->CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS3,&options,sizeof(options));
    if(FAILED(capability))return capability;
    if(options.ViewInstancingTier==D3D12_VIEW_INSTANCING_TIER_NOT_SUPPORTED)return DXGI_ERROR_UNSUPPORTED;
    Stream s;
    s.root.value=source.pRootSignature;s.vs.value=source.VS;s.ps.value=source.PS;
    s.ds.value=source.DS;s.hs.value=source.HS;s.gs.value=source.GS;s.streamOutput.value=source.StreamOutput;
    s.blend.value=source.BlendState;s.sampleMask.value=source.SampleMask;
    s.raster.value=source.RasterizerState;s.depth.value=source.DepthStencilState;
    s.input.value=source.InputLayout;s.stripCut.value=source.IBStripCutValue;s.topology.value=source.PrimitiveTopologyType;
    s.targets.value.NumRenderTargets=source.NumRenderTargets;
    for(UINT i=0;i<source.NumRenderTargets;++i)s.targets.value.RTFormats[i]=source.RTVFormats[i];
    s.depthFormat.value=source.DSVFormat;s.samples.value=source.SampleDesc;
    s.nodeMask.value=source.NodeMask;s.flags.value=source.Flags;
    // A cached monoscopic PSO cannot describe the changed shaders/view layout.
    // Every other classic descriptor field is preserved in the stream above.
    D3D12_PIPELINE_STATE_STREAM_DESC stream{offsetof(Stream,views),&s};
    return CreateViewInstancedPipelineStream(device,stream,source.VS,layout,result);
}

namespace {
// Two passes over the same pointers: the counter sizes every array so nothing reallocates while
// the copy hands out addresses into them.
struct PartCounter {
    size_t shaders{},inputs{},outputs{},strides{},views{},names{};
    void Shader(const D3D12_SHADER_BYTECODE& s){if(s.pShaderBytecode && s.BytecodeLength)++shaders;}
    void Input(const D3D12_INPUT_LAYOUT_DESC& d) {
        if(!d.pInputElementDescs)return;inputs+=d.NumElements;
        for(UINT i=0;i<d.NumElements;++i)names+=d.pInputElementDescs[i].SemanticName?std::strlen(d.pInputElementDescs[i].SemanticName)+1:0;
    }
    void Output(const D3D12_STREAM_OUTPUT_DESC& d) {
        if(d.pSODeclaration){outputs+=d.NumEntries;for(UINT i=0;i<d.NumEntries;++i)names+=d.pSODeclaration[i].SemanticName?std::strlen(d.pSODeclaration[i].SemanticName)+1:0;}
        if(d.pBufferStrides)strides+=d.NumStrides;
    }
    void Views(const D3D12_VIEW_INSTANCING_DESC& d){if(d.pViewInstanceLocations)views+=d.ViewInstanceCount;}
    void Reserve(OwnedPipelineParts& p)const{p.shaders.reserve(shaders);p.inputs.reserve(inputs);p.outputs.reserve(outputs);
        p.strides.reserve(strides);p.views.reserve(views);p.names.reserve(names);}
};
const char* OwnName(OwnedPipelineParts& p,const char* name) {
    if(!name)return nullptr;
    const size_t at=p.names.size();p.names.insert(p.names.end(),name,name+std::strlen(name)+1);
    return p.names.data()+at;
}
void OwnShader(OwnedPipelineParts& p,D3D12_SHADER_BYTECODE& s) {
    if(!s.pShaderBytecode || !s.BytecodeLength){s={};return;}
    const auto* bytes=static_cast<const uint8_t*>(s.pShaderBytecode);
    p.shaders.emplace_back(bytes,bytes+s.BytecodeLength);s.pShaderBytecode=p.shaders.back().data();
}
void OwnInput(OwnedPipelineParts& p,D3D12_INPUT_LAYOUT_DESC& d) {
    if(!d.pInputElementDescs || !d.NumElements){d={};return;}
    const size_t at=p.inputs.size();
    for(UINT i=0;i<d.NumElements;++i){auto e=d.pInputElementDescs[i];e.SemanticName=OwnName(p,e.SemanticName);p.inputs.push_back(e);}
    d.pInputElementDescs=p.inputs.data()+at;
}
void OwnOutput(OwnedPipelineParts& p,D3D12_STREAM_OUTPUT_DESC& d) {
    if(d.pSODeclaration && d.NumEntries) {
        const size_t at=p.outputs.size();
        for(UINT i=0;i<d.NumEntries;++i){auto e=d.pSODeclaration[i];e.SemanticName=OwnName(p,e.SemanticName);p.outputs.push_back(e);}
        d.pSODeclaration=p.outputs.data()+at;
    } else {d.pSODeclaration=nullptr;d.NumEntries=0;}
    if(d.pBufferStrides && d.NumStrides) {
        const size_t at=p.strides.size();p.strides.insert(p.strides.end(),d.pBufferStrides,d.pBufferStrides+d.NumStrides);
        d.pBufferStrides=p.strides.data()+at;
    } else {d.pBufferStrides=nullptr;d.NumStrides=0;}
}
void OwnViews(OwnedPipelineParts& p,D3D12_VIEW_INSTANCING_DESC& d) {
    if(!d.pViewInstanceLocations || !d.ViewInstanceCount){d.pViewInstanceLocations=nullptr;return;}
    const size_t at=p.views.size();p.views.insert(p.views.end(),d.pViewInstanceLocations,d.pViewInstanceLocations+d.ViewInstanceCount);
    d.pViewInstanceLocations=p.views.data()+at;
}
bool IsShaderPart(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE type) {
    switch(type) {
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS:case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS:case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DS:
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_HS:case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_GS:case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CS:
    case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS:case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS:return true;
    default:return false;
    }
}
}

bool OwnPipelineStream(const D3D12_PIPELINE_STATE_STREAM_DESC& source,OwnedPipelineStream& output) {
    PartCounter count;
    if(!Walk(source,[&](auto type,size_t,const uint8_t* value) {
        if(IsShaderPart(type)){D3D12_SHADER_BYTECODE s;std::memcpy(&s,value,sizeof(s));count.Shader(s);}
        else if(type==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT){D3D12_INPUT_LAYOUT_DESC d;std::memcpy(&d,value,sizeof(d));count.Input(d);}
        else if(type==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_STREAM_OUTPUT){D3D12_STREAM_OUTPUT_DESC d;std::memcpy(&d,value,sizeof(d));count.Output(d);}
        else if(type==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VIEW_INSTANCING){D3D12_VIEW_INSTANCING_DESC d;std::memcpy(&d,value,sizeof(d));count.Views(d);}
        return true;
    }))return false;
    const auto* bytes=static_cast<const uint8_t*>(source.pPipelineStateSubobjectStream);
    output.stream.assign(bytes,bytes+source.SizeInBytes);
    output.offsets={};output.parts={};count.Reserve(output.parts);
    auto& p=output.parts;auto* stream=output.stream.data();
    return Walk(source,[&](auto type,size_t offset,const uint8_t*) {
        auto* value=stream+offset;
        if(IsShaderPart(type)){D3D12_SHADER_BYTECODE s;std::memcpy(&s,value,sizeof(s));OwnShader(p,s);std::memcpy(value,&s,sizeof(s));}
        else if(type==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT){D3D12_INPUT_LAYOUT_DESC d;std::memcpy(&d,value,sizeof(d));OwnInput(p,d);std::memcpy(value,&d,sizeof(d));}
        else if(type==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_STREAM_OUTPUT){D3D12_STREAM_OUTPUT_DESC d;std::memcpy(&d,value,sizeof(d));OwnOutput(p,d);std::memcpy(value,&d,sizeof(d));}
        else if(type==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VIEW_INSTANCING){D3D12_VIEW_INSTANCING_DESC d;std::memcpy(&d,value,sizeof(d));OwnViews(p,d);std::memcpy(value,&d,sizeof(d));}
        else if(type==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CACHED_PSO){const D3D12_CACHED_PIPELINE_STATE none{};std::memcpy(value,&none,sizeof(none));}
        switch(type) {
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS:output.offsets.vs=offset;break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS:output.offsets.ps=offset;break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CS:output.offsets.cs=offset;break;
        case D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CACHED_PSO:output.offsets.cached=offset;break;
        default:break;
        }
        return true;
    });
}
bool OwnGraphicsDesc(const D3D12_GRAPHICS_PIPELINE_STATE_DESC& source,OwnedGraphicsDesc& output) {
    PartCounter count;
    for(const auto* s:{&source.VS,&source.PS,&source.DS,&source.HS,&source.GS})count.Shader(*s);
    count.Input(source.InputLayout);count.Output(source.StreamOutput);
    output.desc=source;output.parts={};count.Reserve(output.parts);
    auto& d=output.desc;auto& p=output.parts;
    for(auto* s:{&d.VS,&d.PS,&d.DS,&d.HS,&d.GS})OwnShader(p,*s);
    OwnInput(p,d.InputLayout);OwnOutput(p,d.StreamOutput);d.CachedPSO={};
    return true;
}
}
