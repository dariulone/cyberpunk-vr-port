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
}
