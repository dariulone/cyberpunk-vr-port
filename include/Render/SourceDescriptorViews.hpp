#pragma once
#include <cstddef>
#include "Render/StereoDescriptorStore.hpp"
#include <bit>
#include <climits>
#include <d3d12.h>

namespace cvr::stereo::descriptor_views {
// Observed creation metadata only. Alignment and heap/node flags were not
// recorded; no resource identity, allocation, state or lifetime is established.
struct ResourceMetadata {
    bool described{},heapDescribed{};
    D3D12_RESOURCE_DIMENSION dimension{};DXGI_FORMAT format{};UINT64 width{};
    UINT height{};UINT16 depthOrArraySize{},mipLevels{};
    D3D12_RESOURCE_FLAGS flags{};D3D12_TEXTURE_LAYOUT layout{};DXGI_SAMPLE_DESC samples{};
    D3D12_HEAP_TYPE heapType{};D3D12_CPU_PAGE_PROPERTY cpuPage{};D3D12_MEMORY_POOL memoryPool{};
    UINT64 bufferGpuVA{};
};
struct CbvResult {
    D3D12_CONSTANT_BUFFER_VIEW_DESC view{};
    // Optional numeric origin labels only, not resource/allocation permission.
    uint64_t resourceCookie{},originId{};
};
struct SrvResult {
    uint64_t resourceCookie{};bool implicit{};
    D3D12_SHADER_RESOURCE_VIEW_DESC view{};ResourceMetadata metadata;
    const D3D12_SHADER_RESOURCE_VIEW_DESC* Description()const{return implicit?nullptr:&view;}
};
struct UavResult {
    uint64_t resourceCookie{},counterCookie{};bool implicit{};
    D3D12_UNORDERED_ACCESS_VIEW_DESC view{};
    const D3D12_UNORDERED_ACCESS_VIEW_DESC* Description()const{return implicit?nullptr:&view;}
};
struct SamplerResult {D3D12_SAMPLER_DESC view{};};
// Caller certifies the exact current address and its backing lifetime separately.
// This value is not acquired from optional buffer metadata or a cookie lookup.
struct AccelerationStructureAddressCertificate {D3D12_GPU_VIRTUAL_ADDRESS location{};};
namespace detail {
using descriptors::Value;
inline bool U32(uint64_t value){return value<=UINT_MAX;}
inline float Float(uint64_t value){return std::bit_cast<float>(uint32_t(value));}
inline bool Format(uint64_t value){
    return value<=115 || (value>=130 && value<=132) || (value>=189 && value<=191);
}
inline bool TypedFormat(uint64_t value){
    if(!Format(value) || value==DXGI_FORMAT_UNKNOWN)return false;
    switch(DXGI_FORMAT(value)){
    case DXGI_FORMAT_R32G32B32A32_TYPELESS:case DXGI_FORMAT_R32G32B32_TYPELESS:case DXGI_FORMAT_R16G16B16A16_TYPELESS:
    case DXGI_FORMAT_R32G32_TYPELESS:case DXGI_FORMAT_R32G8X24_TYPELESS:case DXGI_FORMAT_R10G10B10A2_TYPELESS:
    case DXGI_FORMAT_R8G8B8A8_TYPELESS:case DXGI_FORMAT_R16G16_TYPELESS:case DXGI_FORMAT_R32_TYPELESS:
    case DXGI_FORMAT_R24G8_TYPELESS:case DXGI_FORMAT_R8G8_TYPELESS:case DXGI_FORMAT_R16_TYPELESS:case DXGI_FORMAT_R8_TYPELESS:
    case DXGI_FORMAT_BC1_TYPELESS:case DXGI_FORMAT_BC2_TYPELESS:case DXGI_FORMAT_BC3_TYPELESS:case DXGI_FORMAT_BC4_TYPELESS:
    case DXGI_FORMAT_BC5_TYPELESS:case DXGI_FORMAT_B8G8R8A8_TYPELESS:case DXGI_FORMAT_B8G8R8X8_TYPELESS:
    case DXGI_FORMAT_BC6H_TYPELESS:case DXGI_FORMAT_BC7_TYPELESS:return false;
    default:return true;
    }
}
inline bool Words(const Value& value,uint32_t used){
    for(UINT i=0;i<value.size();++i)if(!(used&(uint32_t(1)<<i)) && value[i])return false;
    return true;
}
inline bool Payload(const Value& value,UINT last,uint32_t wide=0){
    for(UINT i=7;i<=last;++i)if(!(wide&(uint32_t(1)<<i)) && !U32(value[i]))return false;
    for(UINT i=last+1;i<=12;++i)if(value[i])return false;
    return true;
}
inline bool Span32(uint64_t first,uint64_t count,bool remaining=true){
    return U32(first) && U32(count) && ((remaining && count==UINT_MAX) || first+count<=uint64_t(UINT_MAX)+1);
}
inline bool Buffer(const Value& value,UINT flagWord){
    if(value[flagWord]>1 || value[7]>UINT64_MAX-value[8])return false;
    if(value[flagWord])return value[4]==DXGI_FORMAT_R32_TYPELESS && !value[9];
    return value[9]?value[4]==DXGI_FORMAT_UNKNOWN:TypedFormat(value[4]);
}
inline bool Mapping(uint64_t value){
    if(!U32(value) || !(value&0x1000) || (value&~uint64_t(0x1FFF)))return false;
    for(UINT i=0;i<4;++i)if(((value>>(i*3))&7)>5)return false;
    return true;
}
inline bool Metadata(const Value& value,ResourceMetadata& output){
    ResourceMetadata result;
    if(value[1]&~uint64_t(15) || bool(value[1]&1)!=bool(value[2]) || value[3] ||
        ((value[1]&12) && !(value[1]&1)) || ((value[1]&8) && !(value[1]&4)))return false;
    if(!(value[1]&4)){
        for(UINT i=13;i<20;++i)if(value[i])return false;
        output=result;return true;
    }
    const auto dimension=uint32_t(value[13]),format=uint32_t(value[13]>>32),layout=uint32_t(value[16]>>32);
    if(dimension<D3D12_RESOURCE_DIMENSION_BUFFER || dimension>D3D12_RESOURCE_DIMENSION_TEXTURE3D || !Format(format) ||
        (uint32_t(value[16])&~uint32_t(0x1FF)) || layout>D3D12_TEXTURE_LAYOUT_64KB_STANDARD_SWIZZLE ||
        (dimension!=D3D12_RESOURCE_DIMENSION_BUFFER && value[19]))return false;
    result.described=true;result.dimension=D3D12_RESOURCE_DIMENSION(dimension);result.format=DXGI_FORMAT(format);result.width=value[14];
    result.height=uint32_t(value[15]);result.depthOrArraySize=uint16_t(value[15]>>32);result.mipLevels=uint16_t(value[15]>>48);
    result.flags=D3D12_RESOURCE_FLAGS(uint32_t(value[16]));result.layout=D3D12_TEXTURE_LAYOUT(layout);
    result.samples={uint32_t(value[17]),uint32_t(value[17]>>32)};result.bufferGpuVA=value[19];
    if(value[1]&8){
        const auto type=uint16_t(value[18]),page=uint16_t(value[18]>>16),pool=uint16_t(value[18]>>32);
        if(value[18]>>48 || type<1 || type>5 || page>4 || pool>2)return false;
        result.heapDescribed=true;result.heapType=D3D12_HEAP_TYPE(type);result.cpuPage=D3D12_CPU_PAGE_PROPERTY(page);result.memoryPool=D3D12_MEMORY_POOL(pool);
    }else if(value[18])return false;
    output=result;return true;
}
inline bool Filter(uint64_t value){
    if(value&~uint64_t(0x1D5))return false;
    return !(value&0x40) || (value&0x15)==0x15;
}
inline bool ComparisonWord(uint64_t value){
    // Keep ignored values in the SDK enum's representable range, including 0.
    // A negative/sign-extended word is not a canonical valid typed enum value.
    constexpr UINT mask=(UINT(1)<<std::bit_width(UINT(D3D12_COMPARISON_FUNC_ALWAYS)))-1;
    return value<=mask;
}
}
#if CVR_RUST_VRIK
// The decoders: rust/crates/cvr-render/src/descriptor_views.rs (tools/single_pass_tests/source_descriptor_views_test
// runs its cases against them in its Rust variant). Failure leaves output untouched.
static_assert(sizeof(ResourceMetadata)==72 && sizeof(CbvResult)==32 && sizeof(SrvResult)==128 && offsetof(SrvResult,metadata)==56 &&
    sizeof(UavResult)==64 && sizeof(SamplerResult)==52,"mirrored by descriptor_views.rs");
extern "C" {
bool cvr_dv_decode_cbv(const descriptors::Value*,CbvResult*);
bool cvr_dv_decode_srv(const descriptors::Value*,SrvResult*,const uint64_t*);
bool cvr_dv_decode_uav(const descriptors::Value*,UavResult*);
bool cvr_dv_decode_sampler(const descriptors::Value*,SamplerResult*);
}
inline bool DecodeCbv(const descriptors::Value& value,CbvResult& output){return cvr_dv_decode_cbv(&value,&output);}
inline bool DecodeSrv(const descriptors::Value& value,SrvResult& output,
    const AccelerationStructureAddressCertificate* acceleration=nullptr){
    return cvr_dv_decode_srv(&value,&output,acceleration?&acceleration->location:nullptr);
}
inline bool DecodeUav(const descriptors::Value& value,UavResult& output){return cvr_dv_decode_uav(&value,&output);}
inline bool DecodeSampler(const descriptors::Value& value,SamplerResult& output){return cvr_dv_decode_sampler(&value,&output);}
#else
// A zero kind includes untracked/null CBV pDesc calls and is always unknown.
// Failure leaves output untouched. These decoders make no runtime/API calls.
inline bool DecodeCbv(const descriptors::Value& value,CbvResult& output){
    if(value[0]!=1 || !detail::Words(value,(1u<<0)|(1u<<2)|(1u<<3)|(1u<<4)|(1u<<5)) ||
        bool(value[2])!=bool(value[3]) || !value[4] ||
        (value[4]&255) || !value[5] || value[5]>65536 || (value[5]&255) || value[4]>UINT64_MAX-value[5])return false;
    CbvResult result;result.view={value[4],UINT(value[5])};result.resourceCookie=value[2];result.originId=value[3];output=result;return true;
}
inline bool DecodeSrv(const descriptors::Value& value,SrvResult& output,
    const AccelerationStructureAddressCertificate* acceleration=nullptr){
    if(value[0]!=2)return false;
    SrvResult result;result.resourceCookie=value[2];result.implicit=(value[1]&2)!=0;
    if(!detail::Metadata(value,result.metadata))return false;
    if(result.implicit){
        if(!result.resourceCookie)return false;
        for(UINT i=4;i<=12;++i)if(value[i])return false;
        output=result;return true;
    }
    if(!detail::Format(value[4]) || !detail::U32(value[5]) || !detail::Mapping(value[6]))return false;
    auto& view=result.view;view.Format=DXGI_FORMAT(value[4]);view.ViewDimension=D3D12_SRV_DIMENSION(value[5]);view.Shader4ComponentMapping=UINT(value[6]);
    switch(view.ViewDimension){
    case D3D12_SRV_DIMENSION_BUFFER:
        if(!detail::Payload(value,10,1u<<7) || !detail::Buffer(value,10))return false;
        view.Buffer={value[7],UINT(value[8]),UINT(value[9]),D3D12_BUFFER_SRV_FLAGS(value[10])};break;
    case D3D12_SRV_DIMENSION_TEXTURE1D:
        if(!detail::Payload(value,9) || !detail::Span32(value[7],value[8]))return false;
        view.Texture1D={UINT(value[7]),UINT(value[8]),detail::Float(value[9])};break;
    case D3D12_SRV_DIMENSION_TEXTURE1DARRAY:
        if(!detail::Payload(value,11) || !detail::Span32(value[7],value[8]) || !detail::Span32(value[9],value[10]))return false;
        view.Texture1DArray={UINT(value[7]),UINT(value[8]),UINT(value[9]),UINT(value[10]),detail::Float(value[11])};break;
    case D3D12_SRV_DIMENSION_TEXTURE2D:
        if(!detail::Payload(value,10) || !detail::Span32(value[7],value[8]))return false;
        view.Texture2D={UINT(value[7]),UINT(value[8]),UINT(value[9]),detail::Float(value[10])};break;
    case D3D12_SRV_DIMENSION_TEXTURE2DARRAY:
        if(!detail::Payload(value,12) || !detail::Span32(value[7],value[8]) || !detail::Span32(value[9],value[10]))return false;
        view.Texture2DArray={UINT(value[7]),UINT(value[8]),UINT(value[9]),UINT(value[10]),UINT(value[11]),detail::Float(value[12])};break;
    case D3D12_SRV_DIMENSION_TEXTURE2DMS:
        if(!detail::Payload(value,6))return false;break;
    case D3D12_SRV_DIMENSION_TEXTURE2DMSARRAY:
        if(!detail::Payload(value,8) || !detail::Span32(value[7],value[8]))return false;
        view.Texture2DMSArray={UINT(value[7]),UINT(value[8])};break;
    case D3D12_SRV_DIMENSION_TEXTURE3D:
        if(!detail::Payload(value,9) || !detail::Span32(value[7],value[8]))return false;
        view.Texture3D={UINT(value[7]),UINT(value[8]),detail::Float(value[9])};break;
    case D3D12_SRV_DIMENSION_TEXTURECUBE:
        if(!detail::Payload(value,9) || !detail::Span32(value[7],value[8]))return false;
        view.TextureCube={UINT(value[7]),UINT(value[8]),detail::Float(value[9])};break;
    case D3D12_SRV_DIMENSION_TEXTURECUBEARRAY:
        if(!detail::Payload(value,11) || !detail::Span32(value[7],value[8]) ||
            (value[10]!=UINT_MAX && value[9]+value[10]*6>uint64_t(UINT_MAX)+1))return false;
        view.TextureCubeArray={UINT(value[7]),UINT(value[8]),UINT(value[9]),UINT(value[10]),detail::Float(value[11])};break;
    case D3D12_SRV_DIMENSION_RAYTRACING_ACCELERATION_STRUCTURE:
        if(!detail::Payload(value,7,1u<<7) || result.resourceCookie || value[1] || value[4]!=DXGI_FORMAT_UNKNOWN ||
            !acceleration || !value[7] || value[7]%D3D12_RAYTRACING_ACCELERATION_STRUCTURE_BYTE_ALIGNMENT || acceleration->location!=value[7])return false;
        view.RaytracingAccelerationStructure.Location=value[7];output=result;return true;
    default:return false;
    }
    if(view.ViewDimension!=D3D12_SRV_DIMENSION_BUFFER && !detail::TypedFormat(value[4]))return false;
    output=result;return true;
}
inline bool DecodeUav(const descriptors::Value& value,UavResult& output){
    if(value[0]!=3 || (value[1]&~uint64_t(3)) || bool(value[1]&1)!=bool(value[2]) || value[6])return false;
    for(UINT i=12;i<20;++i)if(value[i])return false;
    UavResult result;result.resourceCookie=value[2];result.counterCookie=value[3];result.implicit=(value[1]&2)!=0;
    if(result.implicit){
        if(!result.resourceCookie || result.counterCookie)return false;
        for(UINT i=4;i<=11;++i)if(value[i])return false;
        output=result;return true;
    }
    if(!detail::Format(value[4]) || !detail::U32(value[5]))return false;
    auto& view=result.view;view.Format=DXGI_FORMAT(value[4]);view.ViewDimension=D3D12_UAV_DIMENSION(value[5]);
    if(result.counterCookie && (!result.resourceCookie || view.ViewDimension!=D3D12_UAV_DIMENSION_BUFFER))return false;
    switch(view.ViewDimension){
    case D3D12_UAV_DIMENSION_BUFFER:
        if(!detail::Payload(value,11,(1u<<7)|(1u<<10)) || !detail::Buffer(value,11))return false;
        if(result.counterCookie){
            if(!value[9] || value[4]!=DXGI_FORMAT_UNKNOWN || value[11] || value[10]%D3D12_UAV_COUNTER_PLACEMENT_ALIGNMENT || value[10]>UINT64_MAX-4)return false;
        }else if(value[10])return false;
        view.Buffer={value[7],UINT(value[8]),UINT(value[9]),value[10],D3D12_BUFFER_UAV_FLAGS(value[11])};break;
    case D3D12_UAV_DIMENSION_TEXTURE1D:
        if(!detail::Payload(value,7))return false;view.Texture1D={UINT(value[7])};break;
    case D3D12_UAV_DIMENSION_TEXTURE1DARRAY:
        if(!detail::Payload(value,9) || !detail::Span32(value[8],value[9]))return false;
        view.Texture1DArray={UINT(value[7]),UINT(value[8]),UINT(value[9])};break;
    case D3D12_UAV_DIMENSION_TEXTURE2D:
        if(!detail::Payload(value,8))return false;view.Texture2D={UINT(value[7]),UINT(value[8])};break;
    case D3D12_UAV_DIMENSION_TEXTURE2DARRAY:
        if(!detail::Payload(value,10) || !detail::Span32(value[8],value[9]))return false;
        view.Texture2DArray={UINT(value[7]),UINT(value[8]),UINT(value[9]),UINT(value[10])};break;
    case D3D12_UAV_DIMENSION_TEXTURE3D:
        if(!detail::Payload(value,9) || !detail::Span32(value[8],value[9]))return false;
        view.Texture3D={UINT(value[7]),UINT(value[8]),UINT(value[9])};break;
    default:return false;
    }
    if(view.ViewDimension!=D3D12_UAV_DIMENSION_BUFFER && !detail::TypedFormat(value[4]))return false;
    output=result;return true;
}
inline bool DecodeSampler(const descriptors::Value& value,SamplerResult& output){
    if(value[0]!=4 || !detail::Words(value,1u|(((1u<<17)-1)&~15u)) || !detail::Filter(value[4]))return false;
    for(UINT i=4;i<=16;++i)if(i==10?!detail::ComparisonWord(value[i]):!detail::U32(value[i]))return false;
    for(UINT i=5;i<=7;++i)if(value[i]<D3D12_TEXTURE_ADDRESS_MODE_WRAP || value[i]>D3D12_TEXTURE_ADDRESS_MODE_MIRROR_ONCE)return false;
    if((value[4]&0x40) && (!value[9] || value[9]>16))return false;
    if((value[4]&0x180)==0x80 && (value[10]<D3D12_COMPARISON_FUNC_NEVER || value[10]>D3D12_COMPARISON_FUNC_ALWAYS))return false;
    SamplerResult result;auto& view=result.view;
    view.Filter=D3D12_FILTER(value[4]);view.AddressU=D3D12_TEXTURE_ADDRESS_MODE(value[5]);view.AddressV=D3D12_TEXTURE_ADDRESS_MODE(value[6]);view.AddressW=D3D12_TEXTURE_ADDRESS_MODE(value[7]);
    view.MipLODBias=detail::Float(value[8]);view.MaxAnisotropy=UINT(value[9]);view.ComparisonFunc=D3D12_COMPARISON_FUNC(value[10]);
    for(UINT i=0;i<4;++i)view.BorderColor[i]=detail::Float(value[11+i]);view.MinLOD=detail::Float(value[15]);view.MaxLOD=detail::Float(value[16]);
    if(!(view.MaxLOD>=view.MinLOD))return false;
    if(view.AddressU==D3D12_TEXTURE_ADDRESS_MODE_BORDER || view.AddressV==D3D12_TEXTURE_ADDRESS_MODE_BORDER || view.AddressW==D3D12_TEXTURE_ADDRESS_MODE_BORDER)
        for(const auto component:view.BorderColor)if(!(component>=0 && component<=1))return false;
    output=result;return true;
}
#endif
}
