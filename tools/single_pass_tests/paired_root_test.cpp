#include "Render/StereoPairedRoot.hpp"
#include <wrl/client.h>
#include <algorithm>
#include <cstdio>
#include <stdexcept>
#include <string>
using namespace cvr::stereo;
using Microsoft::WRL::ComPtr;
static void Require(bool v){if(!v)throw std::runtime_error("Paired root assertion");}
static ComPtr<ID3DBlob> Serialize(const D3D12_ROOT_SIGNATURE_DESC1& desc) {
    D3D12_VERSIONED_ROOT_SIGNATURE_DESC version{};version.Version=D3D_ROOT_SIGNATURE_VERSION_1_1;version.Desc_1_1=desc;
    ComPtr<ID3DBlob> blob,error;Require(SUCCEEDED(D3D12SerializeVersionedRootSignature(&version,&blob,&error)));return blob;
}
// "trace": the canonical plan's bytes and locations hashed, so the C++ and the Rust build compare (cmake/ParityTrace.cmake).
int main(int argc,char** argv) try {
    const bool trace=argc>1 && std::string(argv[1])=="trace";
    D3D12_DESCRIPTOR_RANGE1 ranges[]{{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,24,8,0,D3D12_DESCRIPTOR_RANGE_FLAG_DATA_STATIC,3},
        {D3D12_DESCRIPTOR_RANGE_TYPE_SRV,1,37,0,D3D12_DESCRIPTOR_RANGE_FLAG_DESCRIPTORS_VOLATILE,D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND}};
    D3D12_ROOT_PARAMETER1 params[3]{};
    params[0].ParameterType=D3D12_ROOT_PARAMETER_TYPE_CBV;params[0].Descriptor={1,0,D3D12_ROOT_DESCRIPTOR_FLAG_DATA_VOLATILE};
    params[0].ShaderVisibility=D3D12_SHADER_VISIBILITY_VERTEX;
    params[1].ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;params[1].DescriptorTable={2,ranges};
    params[1].ShaderVisibility=D3D12_SHADER_VISIBILITY_PIXEL;
    params[2].ParameterType=D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;params[2].Constants={9,0,3};
    D3D12_STATIC_SAMPLER_DESC sampler{};sampler.Filter=D3D12_FILTER_MIN_MAG_MIP_POINT;
    sampler.AddressU=sampler.AddressV=sampler.AddressW=D3D12_TEXTURE_ADDRESS_MODE_CLAMP;sampler.ComparisonFunc=D3D12_COMPARISON_FUNC_ALWAYS;
    sampler.MaxLOD=D3D12_FLOAT32_MAX;sampler.ShaderRegister=17;
    D3D12_ROOT_SIGNATURE_DESC1 desc{3,params,1,&sampler,D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT};
    auto blob=Serialize(desc);
    std::vector<PairedBinding> pairs{{D3D12_DESCRIPTOR_RANGE_TYPE_CBV,D3D12_SHADER_VISIBILITY_VERTEX,1,100},
        {D3D12_DESCRIPTOR_RANGE_TYPE_SRV,D3D12_SHADER_VISIBILITY_PIXEL,37,101}};
    PairedRootPlan plan;Require(SUCCEEDED(BuildPairedRoot(blob->GetBufferPointer(),blob->GetBufferSize(),pairs,plan)));
    Require(plan.originalParameters==3 && plan.rootDwords==8 && plan.parameters==std::array<UINT,2>{3,4});
    Require(plan.descriptors==std::array<UINT,2>{2,2});
    Require(plan.bindings[0].sourceType==D3D12_ROOT_PARAMETER_TYPE_CBV && plan.bindings[0].sourceParameter==0);
    Require(plan.bindings[1].sourceType==D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE && plan.bindings[1].sourceOffset==27);
    ComPtr<ID3D12VersionedRootSignatureDeserializer> reader;
    Require(SUCCEEDED(D3D12CreateVersionedRootSignatureDeserializer(plan.serialized.data(),plan.serialized.size(),IID_PPV_ARGS(&reader))));
    const auto& generated=reader->GetUnconvertedRootSignatureDesc()->Desc_1_1;
    Require(generated.NumParameters==5 && generated.Flags==desc.Flags && generated.NumStaticSamplers==1);
    Require(generated.pStaticSamplers[0].ShaderRegister==17 && generated.pParameters[2].Constants.Num32BitValues==3);
    Require(generated.pParameters[1].DescriptorTable.pDescriptorRanges[0].Flags==D3D12_DESCRIPTOR_RANGE_FLAG_DATA_STATIC);
    Require(generated.pParameters[4].DescriptorTable.pDescriptorRanges[0].BaseShaderRegister==74);
    const auto valid=plan.serialized;
    uint64_t hash=1469598103934665603ull;
    const auto mix=[&](const void* p,size_t n){for(size_t i=0;i<n;++i){hash^=static_cast<const uint8_t*>(p)[i];hash*=1099511628211ull;}};
    mix(plan.serialized.data(),plan.serialized.size());mix(plan.bindings.data(),plan.bindings.size()*sizeof(PairedBindingLocation));
    mix(plan.parameters.data(),8);mix(plan.descriptors.data(),8);mix(&plan.originalParameters,4);mix(&plan.rootDwords,4);
    std::reverse(pairs.begin(),pairs.end());Require(SUCCEEDED(BuildPairedRoot(blob->GetBufferPointer(),blob->GetBufferSize(),pairs,plan)) && plan.serialized==valid);
    auto reject=[&](std::vector<PairedBinding> bad) {
        Require(FAILED(BuildPairedRoot(blob->GetBufferPointer(),blob->GetBufferSize(),bad,plan)) && plan.serialized==valid);
    };
    auto bad=pairs;bad.push_back(pairs[0]);reject(bad);
    bad=pairs;bad[0].sourceRegister=99;reject(bad);
    bad=pairs;bad[0].privateSpace=0;reject(bad);
    bad=pairs;bad[0].visibility=D3D12_SHADER_VISIBILITY_ALL;reject(bad);
    bad=pairs;bad[0].type=D3D12_DESCRIPTOR_RANGE_TYPE_UAV;reject(bad);
    bad=pairs;bad[0].sourceRegister=UINT_MAX;reject(bad);
    reject({{D3D12_DESCRIPTOR_RANGE_TYPE_CBV,D3D12_SHADER_VISIBILITY_VERTEX,9,100}});
    params[2].Constants.Num32BitValues=61;blob=Serialize(desc);reject(pairs); // 64 existing DWORDs, no room for two banks.
    params[2].Constants={2,100,1};blob=Serialize(desc);reject(pairs); // private b2 overlaps.
    params[2].Constants={9,0,3};desc.Flags|=D3D12_ROOT_SIGNATURE_FLAG_DENY_PIXEL_SHADER_ROOT_ACCESS;
    blob=Serialize(desc);reject(pairs);
    if(trace){std::printf("paired_root trace %016llx\n",static_cast<unsigned long long>(hash));return 0;}
    std::puts("PASS paired root locations, compact banks, original flags/static samplers, canonical ordering and rejection paths");
    return 0;
}catch(const std::exception& e){std::fprintf(stderr,"%s\n",e.what());return 1;}
