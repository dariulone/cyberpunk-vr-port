#include "Render/StereoPairedRoot.hpp"
#if !CVR_RUST_VRIK
// The C++ build's builder; the Rust build's is rust/crates/cvr-render/src/paired_root.rs (the header calls it).
#include <wrl/client.h>
#include <algorithm>
#include <tuple>

namespace cvr::stereo {
namespace {
bool Visible(D3D12_SHADER_VISIBILITY declared,D3D12_SHADER_VISIBILITY stage) {
    return declared==D3D12_SHADER_VISIBILITY_ALL || declared==stage;
}
bool Matches(D3D12_ROOT_PARAMETER_TYPE root,D3D12_DESCRIPTOR_RANGE_TYPE type) {
    return (root==D3D12_ROOT_PARAMETER_TYPE_CBV && type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV) ||
           (root==D3D12_ROOT_PARAMETER_TYPE_SRV && type==D3D12_DESCRIPTOR_RANGE_TYPE_SRV);
}
bool Overlaps(UINT base,UINT count,UINT target,UINT length) {
    const uint64_t end=count==UINT_MAX?uint64_t(UINT_MAX)+1:uint64_t(base)+count;
    return uint64_t(target)<end && uint64_t(base)<uint64_t(target)+length;
}
}
HRESULT BuildPairedRoot(const void* bytes,size_t length,std::span<const PairedBinding> bindings,PairedRootPlan& output) {
    if(!bytes || !length || length>1024*1024 || bindings.size()>512)return E_INVALIDARG;
    Microsoft::WRL::ComPtr<ID3D12VersionedRootSignatureDeserializer> reader;
    auto hr=D3D12CreateVersionedRootSignatureDeserializer(bytes,length,IID_PPV_ARGS(&reader));
    if(FAILED(hr))return hr;
    const D3D12_VERSIONED_ROOT_SIGNATURE_DESC* version{};
    hr=reader->GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1,&version);
    if(FAILED(hr))return hr;
    const auto& source=version->Desc_1_1;
    if(source.NumParameters>64 || (source.Flags&D3D12_ROOT_SIGNATURE_FLAG_LOCAL_ROOT_SIGNATURE))return E_INVALIDARG;
    PairedRootPlan result;result.originalParameters=source.NumParameters;
    std::vector<D3D12_ROOT_PARAMETER1> parameters;
    if(source.NumParameters)parameters.assign(source.pParameters,source.pParameters+source.NumParameters);
    for(const auto& p:parameters) {
        switch(p.ParameterType) {
        case D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE:++result.rootDwords;break;
        case D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS:
            if(p.Constants.Num32BitValues>64)return E_INVALIDARG;
            result.rootDwords+=p.Constants.Num32BitValues;break;
        case D3D12_ROOT_PARAMETER_TYPE_CBV:case D3D12_ROOT_PARAMETER_TYPE_SRV:case D3D12_ROOT_PARAMETER_TYPE_UAV:
            result.rootDwords+=2;break;
        default:return E_INVALIDARG;
        }
    }
    std::vector<PairedBinding> ordered(bindings.begin(),bindings.end());
    const auto key=[](const auto& b){return std::tie(b.visibility,b.type,b.sourceRegister,b.privateSpace);};
    std::sort(ordered.begin(),ordered.end(),[&](const auto& a,const auto& b){return key(a)<key(b);});
    std::array<std::vector<D3D12_DESCRIPTOR_RANGE1>,2> banks;
    for(size_t n=0;n<ordered.size();++n) {
        const auto& b=ordered[n];
        if((b.type!=D3D12_DESCRIPTOR_RANGE_TYPE_CBV && b.type!=D3D12_DESCRIPTOR_RANGE_TYPE_SRV) ||
           (b.visibility!=D3D12_SHADER_VISIBILITY_VERTEX && b.visibility!=D3D12_SHADER_VISIBILITY_PIXEL) ||
           !b.privateSpace || b.privateSpace>=65536 || b.sourceRegister>(UINT_MAX-2)/2)return E_INVALIDARG;
        if(n && ordered[n-1].visibility==b.visibility && ordered[n-1].type==b.type &&
           ordered[n-1].sourceRegister==b.sourceRegister)return E_INVALIDARG;
        const UINT stage=b.visibility==D3D12_SHADER_VISIBILITY_VERTEX?0:1;
        const auto deny=stage?D3D12_ROOT_SIGNATURE_FLAG_DENY_PIXEL_SHADER_ROOT_ACCESS:D3D12_ROOT_SIGNATURE_FLAG_DENY_VERTEX_SHADER_ROOT_ACCESS;
        if(source.Flags&deny)return E_INVALIDARG;
        PairedBindingLocation location;location.binding=b;UINT found{};
        for(UINT i=0;i<source.NumParameters;++i) {
            const auto& p=source.pParameters[i];if(!Visible(p.ShaderVisibility,b.visibility))continue;
            if(p.ParameterType==D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE) {
                uint64_t next{};
                for(UINT r=0;r<p.DescriptorTable.NumDescriptorRanges;++r) {
                    const auto& range=p.DescriptorTable.pDescriptorRanges[r];
                    const uint64_t offset=range.OffsetInDescriptorsFromTableStart==D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND?
                        next:range.OffsetInDescriptorsFromTableStart;
                    if(offset>UINT_MAX)return E_INVALIDARG;
                    next=range.NumDescriptors==UINT_MAX?uint64_t(UINT_MAX)+1:offset+range.NumDescriptors;
                    if(range.RangeType!=b.type)continue;
                    if(range.RegisterSpace==b.privateSpace && Overlaps(range.BaseShaderRegister,range.NumDescriptors,b.sourceRegister*2,2))return E_INVALIDARG;
                    if(range.RegisterSpace || !Overlaps(range.BaseShaderRegister,range.NumDescriptors,b.sourceRegister,1))continue;
                    const auto at=offset+b.sourceRegister-range.BaseShaderRegister;if(at>UINT_MAX)return E_INVALIDARG;
                    location.sourceParameter=i;location.sourceOffset=UINT(at);location.sourceType=p.ParameterType;++found;
                }
            } else {
                const bool constants=p.ParameterType==D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
                if(!Matches(p.ParameterType,b.type) && !(constants && b.type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV))continue;
                const auto space=constants?p.Constants.RegisterSpace:p.Descriptor.RegisterSpace;
                const auto reg=constants?p.Constants.ShaderRegister:p.Descriptor.ShaderRegister;
                if(space==b.privateSpace && Overlaps(reg,1,b.sourceRegister*2,2))return E_INVALIDARG;
                if(space || reg!=b.sourceRegister)continue;
                if(constants)return E_NOTIMPL; // No CBV data layout exists for root constants here.
                location.sourceParameter=i;location.sourceOffset=0;location.sourceType=p.ParameterType;++found;
            }
        }
        if(found!=1)return E_INVALIDARG;
        if(result.parameters[stage]==UINT_MAX) {
            result.parameters[stage]=UINT(parameters.size());parameters.emplace_back();++result.rootDwords;
        }
        location.destinationParameter=result.parameters[stage];location.destinationOffset=result.descriptors[stage];
        banks[stage].push_back({b.type,2,b.sourceRegister*2,b.privateSpace,
            D3D12_DESCRIPTOR_RANGE_FLAG_DESCRIPTORS_VOLATILE|D3D12_DESCRIPTOR_RANGE_FLAG_DATA_VOLATILE,result.descriptors[stage]});
        result.descriptors[stage]+=2;result.bindings.push_back(location);
    }
    if(result.rootDwords>64)return E_INVALIDARG;
    for(UINT stage=0;stage<2;++stage)if(result.parameters[stage]!=UINT_MAX) {
        auto& p=parameters[result.parameters[stage]];p.ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
        p.ShaderVisibility=stage?D3D12_SHADER_VISIBILITY_PIXEL:D3D12_SHADER_VISIBILITY_VERTEX;
        p.DescriptorTable={UINT(banks[stage].size()),banks[stage].data()};
    }
    D3D12_VERSIONED_ROOT_SIGNATURE_DESC desc=*version;
    desc.Desc_1_1.NumParameters=UINT(parameters.size());desc.Desc_1_1.pParameters=parameters.data();
    Microsoft::WRL::ComPtr<ID3DBlob> blob,error;
    hr=D3D12SerializeVersionedRootSignature(&desc,&blob,&error);if(FAILED(hr))return hr;
    const auto* begin=static_cast<const uint8_t*>(blob->GetBufferPointer());result.serialized.assign(begin,begin+blob->GetBufferSize());
    output=std::move(result);return S_OK;
}
}
#endif
