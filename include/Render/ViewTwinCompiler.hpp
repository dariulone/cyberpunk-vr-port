#pragma once
// ViewTwinCompiler -- builds the view-instanced twin of a DXIL shader in process (Render/ViewTwinIr.hpp has the
// rewrite): dxcompiler.dll disassembles, the text is rewritten, the DXIL optimizer recomputes the view-ID state
// (-hlsl-dxilload -viewid-state -hlsl-dxilemit), the assembler packs a container and dxil.dll validates and signs it.
// One instance per thread (the DXC objects are not shared).
#include <cstdint>
#include <memory>
#include <string>
#include <vector>

namespace cvr::single_pass {
struct TwinShader {
    bool ok{},unchanged{};
    bool linked{};                    // a pixel shader twin made for this vertex shader (its inputs completed)
    uint32_t loads{},extracts{};
    std::vector<uint8_t> container;   // ok && !unchanged: the signed twin
    std::string error;
};
#if CVR_RUST_VRIK
}
#include "Render/ViewTwinIr.hpp"
// The compiler: rust/crates/cvr-render/src/view_twin_compiler.rs (the result copied out).
extern "C" {
void* cvr_view_twin_compiler_new();
void cvr_view_twin_compiler_free(void*);
bool cvr_view_twin_compiler_load(void*,const wchar_t* folder,size_t len);
void cvr_view_twin_compiler_error(const void*,const char** data,size_t* len);
void* cvr_view_twin_build(const void*,const void* dxil,size_t bytes,const void* vertex,size_t vertexBytes);
void cvr_view_twin_shader_info(const void*,cvr::single_pass::CvrTwinInfo*);
void cvr_view_twin_shader_free(void*);
}
namespace cvr::single_pass {
class ViewTwinCompiler {
public:
    // folder holds dxcompiler.dll and dxil.dll
    bool Load(const std::wstring& folder,std::string& error){
        if(cvr_view_twin_compiler_load(rust_,folder.data(),folder.size()))return true;
        const char* data{};size_t len{};cvr_view_twin_compiler_error(rust_,&data,&len);error.assign(data,len);return false;
    }
    // vertex: for a pixel shader, the vertex shader of the pipeline (its outputs may complete the pixel shader's inputs)
    TwinShader Build(const void* dxil,size_t bytes,const void* vertex=nullptr,size_t vertexBytes=0){
        void* result=cvr_view_twin_build(rust_,dxil,bytes,vertex,vertexBytes);
        CvrTwinInfo info{};cvr_view_twin_shader_info(result,&info);
        TwinShader out;out.ok=info.ok;out.unchanged=info.unchanged;out.linked=info.linked;out.loads=info.loads;out.extracts=info.extracts;
        out.container.assign(info.data,info.data+info.dataLen);out.error.assign(info.error,info.errorLen);
        cvr_view_twin_shader_free(result);return out;
    }
    ViewTwinCompiler():rust_(cvr_view_twin_compiler_new()){}
    ~ViewTwinCompiler(){cvr_view_twin_compiler_free(rust_);}
    ViewTwinCompiler(const ViewTwinCompiler&)=delete;
    ViewTwinCompiler& operator=(const ViewTwinCompiler&)=delete;
private:
    void* rust_;
};
#else
class ViewTwinCompiler {
public:
    // folder holds dxcompiler.dll and dxil.dll
    bool Load(const std::wstring& folder,std::string& error);
    // vertex: for a pixel shader, the vertex shader of the pipeline (its outputs may complete the pixel shader's inputs)
    TwinShader Build(const void* dxil,size_t bytes,const void* vertex=nullptr,size_t vertexBytes=0);
    ViewTwinCompiler();
    ~ViewTwinCompiler();
    ViewTwinCompiler(const ViewTwinCompiler&)=delete;
    ViewTwinCompiler& operator=(const ViewTwinCompiler&)=delete;
private:
    struct Impl;
    std::unique_ptr<Impl> impl;
};
#endif
}
