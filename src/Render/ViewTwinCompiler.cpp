// ViewTwinCompiler -- see include/Render/ViewTwinCompiler.hpp.
#include "Render/ViewTwinCompiler.hpp"
#include "Render/ViewTwinIr.hpp"
#include <windows.h>
#include <unknwn.h>
#include <dxcapi.h>
#include <wrl/client.h>

using Microsoft::WRL::ComPtr;

namespace cvr::single_pass {
struct ViewTwinCompiler::Impl {
    HMODULE compiler{},validatorModule{};
    ComPtr<IDxcCompiler> disassembler;
    ComPtr<IDxcUtils> utils;
    ComPtr<IDxcOptimizer> optimizer;
    ComPtr<IDxcAssembler> assembler;
    ComPtr<IDxcValidator> validator;
};
namespace {
std::string Text(IDxcBlob* blob) {
    if(!blob || !blob->GetBufferSize())return {};
    std::string s(static_cast<const char*>(blob->GetBufferPointer()),blob->GetBufferSize());
    while(!s.empty() && s.back()=='\0')s.pop_back();
    return s;
}
std::string Hr(const char* what,HRESULT hr){char b[96];sprintf_s(b,"%s failed 0x%08X",what,unsigned(hr));return b;}
bool Finish(IDxcOperationResult* result,ComPtr<IDxcBlob>& blob,std::string& error,const char* what) {
    HRESULT status{};
    if(FAILED(result->GetStatus(&status))){error=std::string(what)+": no status";return false;}
    if(FAILED(status)) {
        ComPtr<IDxcBlobEncoding> errors;result->GetErrorBuffer(&errors);
        error=Hr(what,status)+": "+Text(errors.Get()).substr(0,600);
        return false;
    }
    return SUCCEEDED(result->GetResult(&blob)) && blob;
}
}

ViewTwinCompiler::ViewTwinCompiler()=default;
ViewTwinCompiler::~ViewTwinCompiler() {
    if(impl) {
        impl->disassembler.Reset();impl->utils.Reset();impl->optimizer.Reset();impl->assembler.Reset();impl->validator.Reset();
        // The modules stay loaded: DXC keeps process-wide state that must not be torn down under other users.
    }
}

bool ViewTwinCompiler::Load(const std::wstring& folder,std::string& error) {
    impl=std::make_unique<Impl>();
    const std::wstring dir=folder.empty() || folder.back()==L'\\' || folder.back()==L'/'?folder:folder+L"\\";
    // dxil.dll first, by full path: dxcompiler later finds the loaded module by name.
    impl->validatorModule=LoadLibraryExW((dir+L"dxil.dll").c_str(),nullptr,LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    impl->compiler=LoadLibraryExW((dir+L"dxcompiler.dll").c_str(),nullptr,LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if(!impl->compiler || !impl->validatorModule){error="dxcompiler.dll / dxil.dll not found";return false;}
    const auto create=reinterpret_cast<DxcCreateInstanceProc>(GetProcAddress(impl->compiler,"DxcCreateInstance"));
    const auto createValidator=reinterpret_cast<DxcCreateInstanceProc>(GetProcAddress(impl->validatorModule,"DxcCreateInstance"));
    if(!create || !createValidator){error="DxcCreateInstance missing";return false;}
    HRESULT hr;
    if(FAILED(hr=create(CLSID_DxcCompiler,IID_PPV_ARGS(&impl->disassembler)))){error=Hr("DxcCompiler",hr);return false;}
    if(FAILED(hr=create(CLSID_DxcUtils,IID_PPV_ARGS(&impl->utils)))){error=Hr("DxcUtils",hr);return false;}
    if(FAILED(hr=create(CLSID_DxcOptimizer,IID_PPV_ARGS(&impl->optimizer)))){error=Hr("DxcOptimizer",hr);return false;}
    if(FAILED(hr=create(CLSID_DxcAssembler,IID_PPV_ARGS(&impl->assembler)))){error=Hr("DxcAssembler",hr);return false;}
    if(FAILED(hr=createValidator(CLSID_DxcValidator,IID_PPV_ARGS(&impl->validator)))){error=Hr("DxcValidator (dxil.dll)",hr);return false;}
    return true;
}

TwinShader ViewTwinCompiler::Build(const void* dxil,size_t bytes,const void* vertex,size_t vertexBytes) {
    TwinShader out;
    if(!impl || !impl->disassembler){out.error="compiler not loaded";return out;}
    ComPtr<IDxcBlobEncoding> source;
    HRESULT hr=impl->utils->CreateBlob(dxil,UINT32(bytes),DXC_CP_ACP,&source);
    if(FAILED(hr)){out.error=Hr("CreateBlob",hr);return out;}
    ComPtr<IDxcBlobEncoding> disassembly;
    if(FAILED(hr=impl->disassembler->Disassemble(source.Get(),&disassembly)) || !disassembly){out.error=Hr("Disassemble",hr);return out;}
    std::string vertexText;
    if(vertex && vertexBytes) {
        ComPtr<IDxcBlobEncoding> vsSource,vsText;
        if(SUCCEEDED(impl->utils->CreateBlob(vertex,UINT32(vertexBytes),DXC_CP_ACP,&vsSource)) &&
           SUCCEEDED(impl->disassembler->Disassemble(vsSource.Get(),&vsText)) && vsText)vertexText=Text(vsText.Get());
    }
    const IrTwin ir=RewriteForViews(Text(disassembly.Get()),vertexText.empty()?nullptr:&vertexText);
    out.linked=ir.signature;
    out.loads=ir.loads;out.extracts=ir.extracts;
    if(!ir.ok){out.error="rewrite: "+ir.error;return out;}
    if(ir.unchanged){out.ok=out.unchanged=true;return out;}
    ComPtr<IDxcBlobEncoding> text;
    if(FAILED(hr=impl->utils->CreateBlob(ir.text.data(),UINT32(ir.text.size()),DXC_CP_UTF8,&text))){out.error=Hr("CreateBlob",hr);return out;}
    static const wchar_t* passes[]{L"-hlsl-dxilload",L"-viewid-state",L"-hlsl-dxilemit"};
    ComPtr<IDxcBlob> module;ComPtr<IDxcBlobEncoding> messages;
    hr=impl->optimizer->RunOptimizer(text.Get(),passes,UINT32(std::size(passes)),&module,&messages);
    if(FAILED(hr) || !module){out.error=Hr("RunOptimizer",hr)+": "+Text(messages.Get()).substr(0,600);return out;}
    ComPtr<IDxcOperationResult> result;
    if(FAILED(hr=impl->assembler->AssembleToContainer(module.Get(),&result)) || !result){out.error=Hr("AssembleToContainer",hr);return out;}
    ComPtr<IDxcBlob> container;
    if(!Finish(result.Get(),container,out.error,"assemble"))return out;
    result.Reset();
    if(FAILED(hr=impl->validator->Validate(container.Get(),DxcValidatorFlags_InPlaceEdit,&result)) || !result){out.error=Hr("Validate",hr);return out;}
    ComPtr<IDxcBlob> signedContainer;
    if(!Finish(result.Get(),signedContainer,out.error,"validate"))return out;
    // In-place validation signs the container it was given; some versions hand back that same blob.
    const auto* p=static_cast<const uint8_t*>(signedContainer->GetBufferPointer());
    out.container.assign(p,p+signedContainer->GetBufferSize());
    out.ok=true;
    return out;
}
}
