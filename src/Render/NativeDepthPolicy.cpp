#include "Render/NativeDepthPolicy.hpp"
#if !CVR_RUST_VRIK
// The C++ build's policy; the Rust build's is rust/crates/cvr-render/src/depth_policy.rs (the header calls it).
#include <d3dcompiler.h>
#include <d3d12shader.h>
#include <wrl/client.h>
#include <dxcapi.h>
#include <mutex>
#include <filesystem>
#include <cstring>
#include <cstdint>

namespace cvr::stereo {
namespace {
std::mutex reflectionMutex;
DxcCreateInstanceProc ModernReflectionFactory(){
    static const auto factory=[]()->DxcCreateInstanceProc {
        wchar_t path[32768]{};
        const auto n=GetEnvironmentVariableW(L"CVR_STEREO_REFLECTION_DXC",path,32768);
        if(!n || n>=32768 || !std::filesystem::path(path).is_absolute())return nullptr;
        // Keep only the module/factory process-local. COM releases must happen
        // during reflection, never from a static destructor during DLL detach.
        const auto module=LoadLibraryExW(path,nullptr,LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
        if(!module)return nullptr;
        return reinterpret_cast<DxcCreateInstanceProc>(GetProcAddress(module,"DxcCreateInstance"));
    }();
    return factory;
}
bool ExplicitResources(D3D12_SHADER_BYTECODE code,bool singleLayer){
    if(!code.BytecodeLength)return true;
    if(!code.pShaderBytecode || code.BytecodeLength<32 || code.BytecodeLength>16*1024*1024)return false;
    const auto* bytes=static_cast<const uint8_t*>(code.pShaderBytecode);
    auto u32=[&](size_t at){uint32_t v{};std::memcpy(&v,bytes+at,4);return v;};
    if(std::memcmp(bytes,"DXBC",4) || u32(24)!=code.BytecodeLength)return false;
    const auto count=u32(28);if(count>64 || count>(code.BytecodeLength-32)/4)return false;
    bool feature=false,dxil=false;
    for(uint32_t i=0;i<count;++i){
        const auto at=u32(32+4*i);if(at<32+4*count || at>code.BytecodeLength-8)return false;
        const auto size=u32(at+4);if(size>code.BytecodeLength-at-8)return false;
        dxil|=std::memcmp(bytes+at,"DXIL",4)==0;
        if(!std::memcmp(bytes+at,"SFI0",4)){
            if(feature || size!=8)return false;feature=true;uint64_t flags{};std::memcpy(&flags,bytes+at+8,8);
            if(flags&(D3D_SHADER_REQUIRES_RESOURCE_DESCRIPTOR_HEAP_INDEXING | D3D_SHADER_REQUIRES_ATOMIC_INT64_ON_DESCRIPTOR_HEAP_RESOURCE))return false;
            if(singleLayer && (flags&D3D_SHADER_REQUIRES_VIEW_ID))return false;
        }
    }
    return !dxil || feature;
}
bool ReadOnly(D3D12_SHADER_BYTECODE code,bool singleLayer,bool vertex){
    if(!code.BytecodeLength)return !singleLayer || !vertex;
    if(!ExplicitResources(code,singleLayer))return false;
    std::lock_guard lock(reflectionMutex);
    Microsoft::WRL::ComPtr<IDxcUtils> utils;
    Microsoft::WRL::ComPtr<ID3D12ShaderReflection> reflection;
    if(FAILED(D3DReflect(code.pShaderBytecode,code.BytecodeLength,IID_PPV_ARGS(&reflection)))){
        const auto create=ModernReflectionFactory();
        if(!create || FAILED(create(CLSID_DxcUtils,IID_PPV_ARGS(&utils))) || !utils)return false;
        const DxcBuffer buffer{code.pShaderBytecode,code.BytecodeLength,0};
        if(FAILED(utils->CreateReflection(&buffer,IID_PPV_ARGS(&reflection))))return false;
    }
    D3D12_SHADER_DESC desc{};if(FAILED(reflection->GetDesc(&desc)) || desc.BoundResources>4096)return false;
    if(singleLayer){
        if(UINT(D3D12_SHVER_GET_TYPE(desc.Version))!=UINT(vertex?D3D12_SHVER_VERTEX_SHADER:D3D12_SHVER_PIXEL_SHADER) ||
            (reflection->GetRequiresFlags()&D3D_SHADER_REQUIRES_VIEW_ID) || desc.InputParameters>128 || desc.OutputParameters>128)return false;
        for(UINT side=0;side<2;++side)for(UINT i=0;i<(side?desc.OutputParameters:desc.InputParameters);++i){
            D3D12_SIGNATURE_PARAMETER_DESC parameter{};
            const auto hr=side?reflection->GetOutputParameterDesc(i,&parameter):reflection->GetInputParameterDesc(i,&parameter);
            if(FAILED(hr) || !parameter.SemanticName || !_stricmp(parameter.SemanticName,"SV_ViewID"))return false;
            if(vertex && side && (parameter.SystemValueType==D3D_NAME_RENDER_TARGET_ARRAY_INDEX ||
                parameter.SystemValueType==D3D_NAME_VIEWPORT_ARRAY_INDEX || !_stricmp(parameter.SemanticName,"SV_RenderTargetArrayIndex") ||
                !_stricmp(parameter.SemanticName,"SV_ViewportArrayIndex")))return false;
        }
    }
    for(UINT i=0;i<desc.BoundResources;++i){
        D3D12_SHADER_INPUT_BIND_DESC binding{};if(FAILED(reflection->GetResourceBindingDesc(i,&binding)))return false;
        switch(binding.Type){
        case D3D_SIT_CBUFFER:case D3D_SIT_TBUFFER:case D3D_SIT_TEXTURE:case D3D_SIT_SAMPLER:
        case D3D_SIT_STRUCTURED:case D3D_SIT_BYTEADDRESS:case D3D_SIT_RTACCELERATIONSTRUCTURE:break;
        default:return false;
        }
    }
    return true;
}
}
bool ExplicitShaderResources(D3D12_SHADER_BYTECODE code){return ExplicitResources(code,false);}
bool ShaderReadOnly(D3D12_SHADER_BYTECODE code){return ReadOnly(code,false,false);}
bool ShaderSingleLayerReadOnly(D3D12_SHADER_BYTECODE code,bool vertex){return ReadOnly(code,true,vertex);}
}
#endif
