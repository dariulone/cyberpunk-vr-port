// view_twin_test -- the single-pass shader rewrite (src/Render/ViewTwinIr.cpp, ViewTwinCompiler.cpp) over a folder of
// original shaders (<fnv1a>.dxil, e.g. a census dump): every twin must assemble, validate and be signed by dxil.dll,
// and its pipeline-state-validation part must declare that it reads SV_ViewID.
//
//   view_twin_test.exe <folder with dxcompiler.dll and dxil.dll> <shaders> [<out>]
#include "../../src/Render/ViewTwinIr.cpp"
#include "../../src/Render/ViewTwinCompiler.cpp"
#include <chrono>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <map>

namespace fs=std::filesystem;

int wmain(int argc,wchar_t** argv) {
    if(argc<3){std::fprintf(stderr,"view_twin_test <dxc folder> <shaders> [<out>]\n");return 2;}
    cvr::single_pass::ViewTwinCompiler compiler;std::string error;
    if(!compiler.Load(argv[1],error)){std::fprintf(stderr,"load: %s\n",error.c_str());return 2;}
    const fs::path out=argc>3?fs::path(argv[3]):fs::path();
    if(!out.empty())fs::create_directories(out);
    // a second compiler only to read the twins back
    HMODULE dxc=GetModuleHandleW(L"dxcompiler.dll");
    auto create=reinterpret_cast<DxcCreateInstanceProc>(GetProcAddress(dxc,"DxcCreateInstance"));
    ComPtr<IDxcCompiler> reader;ComPtr<IDxcUtils> utils;
    create(CLSID_DxcCompiler,IID_PPV_ARGS(&reader));create(CLSID_DxcUtils,IID_PPV_ARGS(&utils));
    size_t built=0,unchanged=0,failed=0,viewless=0;double ms=0;std::map<std::string,int> reasons;
    for(const auto& entry:fs::directory_iterator(argv[2])) {
        if(entry.path().extension()!=L".dxil")continue;
        std::ifstream f(entry.path(),std::ios::binary);std::vector<uint8_t> bytes((std::istreambuf_iterator<char>(f)),{});
        const auto t0=std::chrono::steady_clock::now();
        const auto twin=compiler.Build(bytes.data(),bytes.size());
        ms+=std::chrono::duration<double,std::milli>(std::chrono::steady_clock::now()-t0).count();
        if(!twin.ok) {
            ++failed;const auto key=twin.error.substr(0,90);++reasons[key];
            if(reasons[key]<=2)std::printf("FAIL %ls: %s\n",entry.path().filename().c_str(),twin.error.substr(0,400).c_str());
            if(!out.empty()) {   // the rewritten text, for dxil_tool's messages
                ComPtr<IDxcBlobEncoding> blob,text;
                utils->CreateBlob(bytes.data(),UINT32(bytes.size()),DXC_CP_ACP,&blob);
                if(SUCCEEDED(reader->Disassemble(blob.Get(),&text)) && text) {
                    const auto ir=cvr::single_pass::RewriteForViews(std::string(static_cast<const char*>(text->GetBufferPointer()),
                                                                                text->GetBufferSize()));
                    if(ir.ok && !ir.unchanged)std::ofstream(out/(entry.path().stem().wstring()+L".fail.ll"),std::ios::binary)<<ir.text;
                }
            }
            continue;
        }
        if(twin.unchanged){++unchanged;continue;}
        ++built;
        ComPtr<IDxcBlobEncoding> blob,text;
        utils->CreateBlob(twin.container.data(),UINT32(twin.container.size()),DXC_CP_ACP,&blob);
        reader->Disassemble(blob.Get(),&text);
        const std::string s=text?std::string(static_cast<const char*>(text->GetBufferPointer()),text->GetBufferSize()):"";
        if(s.find("UsesViewID: true")==std::string::npos){++viewless;std::printf("NO VIEWID %ls\n",entry.path().filename().c_str());}
        if(!out.empty())std::ofstream(out/entry.path().filename(),std::ios::binary).write(
            reinterpret_cast<const char*>(twin.container.data()),std::streamsize(twin.container.size()));
    }
    std::printf("built %zu, unchanged %zu, failed %zu, without view id %zu; %.1f ms per shader\n",built,unchanged,failed,viewless,
                ms/double(std::max<size_t>(1,built+unchanged+failed)));
    for(const auto& [r,n]:reasons)std::printf("  %4d x %s\n",n,r.c_str());
    return failed || viewless?1:0;
}
