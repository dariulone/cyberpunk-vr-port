// view_twin_tests -- the single-pass shader rewrite and twin build (include/Render/ViewTwinIr.hpp,
// include/Render/ViewTwinCompiler.hpp) over a folder of shaders, as a trace: every twin built (alone and, for a pixel
// shader, linked with the corpus's first vertex shader), every rewrite of the disassembly, and rewrites of damaged
// disassemblies (dropped lines, a truncated module) for the failure paths. The C++ build and the Rust port must print
// the same trace (cmake/ParityTrace.cmake).
//
//   view_twin_tests trace                      the configured DXC folder and corpus; one summary line
//   view_twin_tests detail                     the same, one line per result
//   view_twin_tests <dxc folder> <shaders> [detail]
#include "Render/ViewTwinCompiler.hpp"
#include "Render/ViewTwinIr.hpp"
#include <windows.h>
#include <unknwn.h>
#include <dxcapi.h>
#include <wrl/client.h>
#include <algorithm>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <string>
#include <vector>

namespace fs=std::filesystem;
using Microsoft::WRL::ComPtr;
using namespace cvr::single_pass;

namespace {
uint64_t Fnv(const void* data,size_t bytes,uint64_t hash=14695981039346656037ull) {
    const auto* p=static_cast<const uint8_t*>(data);
    for(size_t i=0;i<bytes;++i)hash=(hash^p[i])*1099511628211ull;
    return hash;
}
struct Trace {
    bool detail{};uint64_t hash=14695981039346656037ull;size_t lines{},ok{},unchanged{},linked{},failed{};
    void Line(const std::string& text) {
        hash=Fnv(text.data(),text.size(),hash);hash=Fnv("\n",1,hash);++lines;
        if(detail)std::printf("%s\n",text.c_str());
    }
};
std::string Error(const std::string& error){return error.size()>160?error.substr(0,160):error;}
std::string Ir(const char* what,const std::string& name,const IrTwin& ir,Trace& trace) {
    if(ir.ok)++trace.ok;else ++trace.failed;
    if(ir.unchanged)++trace.unchanged;
    if(ir.signature)++trace.linked;
    char b[200];
    sprintf_s(b,"%s %s ok=%d unchanged=%d signature=%d loads=%u handles=%u extracts=%u text=%016llx/%zu ",what,name.c_str(),
              ir.ok,ir.unchanged,ir.signature,ir.loads,ir.handles,ir.extracts,(unsigned long long)Fnv(ir.text.data(),ir.text.size()),ir.text.size());
    return b+Error(ir.error);
}
std::string Twin(const char* what,const std::string& name,const TwinShader& twin,Trace& trace) {
    if(twin.ok)++trace.ok;else ++trace.failed;
    if(twin.unchanged)++trace.unchanged;
    if(twin.linked)++trace.linked;
    char b[200];
    sprintf_s(b,"%s %s ok=%d unchanged=%d linked=%d loads=%u extracts=%u container=%016llx/%zu ",what,name.c_str(),twin.ok,twin.unchanged,
              twin.linked,twin.loads,twin.extracts,(unsigned long long)Fnv(twin.container.data(),twin.container.size()),twin.container.size());
    return b+Error(twin.error);
}
std::vector<uint8_t> Read(const fs::path& path) {
    std::ifstream f(path,std::ios::binary);return std::vector<uint8_t>((std::istreambuf_iterator<char>(f)),{});
}
}

int wmain(int argc,wchar_t** argv) {
    std::wstring dxc,shaders;Trace trace;
    const std::wstring mode=argc>1?argv[1]:L"";
    if(argc==2 && (mode==L"trace" || mode==L"detail")){dxc=CVR_VIEW_TWIN_DXC;shaders=CVR_VIEW_TWIN_SHADERS;trace.detail=mode==L"detail";}
    else if(argc==3 || argc==4){dxc=argv[1];shaders=argv[2];trace.detail=argc==4;}
    else{std::fprintf(stderr,"view_twin_tests trace|detail | <dxc folder> <shaders> [detail]\n");return 2;}
    {   // the failure paths of an unloaded compiler
        ViewTwinCompiler missing;std::string error="unchanged";
        const bool loaded=missing.Load(L"X:\\cvr-view-twin-missing",error);
        trace.Line("missing load="+std::to_string(loaded)+" "+error);
        trace.Line(Twin("unloaded","-",ViewTwinCompiler{}.Build(nullptr,0),trace));
    }
    ViewTwinCompiler compiler;std::string error="unchanged";
    if(!compiler.Load(dxc,error)){std::fprintf(stderr,"load: %s\n",error.c_str());return 2;}
    trace.Line("load "+error);
    HMODULE module=GetModuleHandleW(L"dxcompiler.dll");
    auto create=reinterpret_cast<DxcCreateInstanceProc>(GetProcAddress(module,"DxcCreateInstance"));
    ComPtr<IDxcCompiler> reader;ComPtr<IDxcUtils> utils;
    if(!create || FAILED(create(CLSID_DxcCompiler,IID_PPV_ARGS(&reader))) || FAILED(create(CLSID_DxcUtils,IID_PPV_ARGS(&utils)))){
        std::fprintf(stderr,"no disassembler\n");return 2;
    }
    const auto disassemble=[&](const std::vector<uint8_t>& bytes) {
        ComPtr<IDxcBlobEncoding> blob,text;
        if(FAILED(utils->CreateBlob(bytes.data(),UINT32(bytes.size()),DXC_CP_ACP,&blob)) || FAILED(reader->Disassemble(blob.Get(),&text)) || !text)return std::string();
        std::string s(static_cast<const char*>(text->GetBufferPointer()),text->GetBufferSize());
        while(!s.empty() && s.back()=='\0')s.pop_back();
        return s;
    };
    std::vector<fs::path> files;
    for(const auto& entry:fs::directory_iterator(shaders))if(entry.path().extension()==L".dxil")files.push_back(entry.path());
    std::sort(files.begin(),files.end());
    if(files.empty()){std::fprintf(stderr,"no shaders in %ls\n",shaders.c_str());return 2;}
    // the corpus's first vertex shader, for the pixel shaders' linked builds
    std::vector<uint8_t> vertex;std::string vertexText;
    for(const auto& path:files){auto bytes=Read(path);auto text=disassemble(bytes);
        if(text.find("!{!\"vs\", i32 6")!=std::string::npos){vertex=std::move(bytes);vertexText=std::move(text);break;}}
    trace.Line("vertex "+std::to_string(vertex.size()));
    for(const auto& path:files) {
        const auto name=path.filename().string();const auto bytes=Read(path);
        trace.Line(Twin("build",name,compiler.Build(bytes.data(),bytes.size()),trace));
        const auto text=disassemble(bytes);
        trace.Line(Ir("ir",name,RewriteForViews(text),trace));
        const bool pixel=text.find("!{!\"ps\", i32 6")!=std::string::npos;
        if(pixel && !vertex.empty()) {
            trace.Line(Twin("linked",name,compiler.Build(bytes.data(),bytes.size(),vertex.data(),vertex.size()),trace));
            trace.Line(Ir("ir-linked",name,RewriteForViews(text,&vertexText),trace));
            const std::string empty;trace.Line(Ir("ir-empty-vertex",name,RewriteForViews(text,&empty),trace));
        }
        // damaged modules: a line dropped at a quarter, half and three quarters, and the first half only
        std::vector<size_t> starts{0};
        for(size_t i=0;i<text.size();++i)if(text[i]=='\n')starts.push_back(i+1);
        for(size_t k=1;k<=3;++k) {
            const size_t line=starts.size()*k/4;if(line+1>=starts.size())continue;
            const auto damaged=text.substr(0,starts[line])+text.substr(starts[line+1]);
            trace.Line(Ir(("drop"+std::to_string(k)).c_str(),name,RewriteForViews(damaged),trace));
        }
        trace.Line(Ir("half",name,RewriteForViews(text.substr(0,text.size()/2)),trace));
    }
    std::printf("view twin trace: %zu shaders, %zu results (%zu ok, %zu unchanged, %zu linked, %zu failed), hash %016llx\n",files.size(),
                trace.lines,trace.ok,trace.unchanged,trace.linked,trace.failed,(unsigned long long)trace.hash);
    return 0;
}
