// Exercises the production shader registry with COM lifetime semantics and deterministic address reuse, and the shader
// census. "trace" prints the counters and the census rows instead, so the C++ build and the Rust build
// (rust/crates/cvr-render/src/shader_patches.rs) can be compared (cmake/ParityTrace.cmake).
//   hidden_area_pipeline_lifetime [signed shader fixture | trace]
#include "Render/HiddenAreaShaders.hpp"
#include "Core/Paths.hpp"
#include <windows.h>
#include <dxgi.h>
#include <atomic>
#include <cstddef>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <new>
#include <stdexcept>
#include <string>

extern "C" int GetMenuMode(){return 0;}
extern "C" {std::atomic<uint32_t> CyberpunkVR_HiddenAreaEnabled{0};}
extern "C" std::atomic<uint32_t> CyberpunkVR_ViewDirPatch,CyberpunkVR_ViewDirPatchLoaded,CyberpunkVR_ViewDirPatchBuilt,
    CyberpunkVR_ViewDirPatchFailed,CyberpunkVR_HiddenAreaPatchRegistered,CyberpunkVR_ShaderCensus,CyberpunkVR_ShaderCensusRecording;
extern "C" std::atomic<uint64_t> CyberpunkVR_ViewDirPatchBinds,CyberpunkVR_ShaderCensusPipelines;
struct CensusRow { uint32_t node; int32_t side; uint32_t kind, x, y, z; uint64_t hash; uint32_t count, bytes; uint64_t pso; };
extern "C" CensusRow CyberpunkVR_ShaderCensusLog[512];
namespace cvr::engine_stereo { bool Enabled(){return false;} }
// The plugin's folders are beside the test: the Rust build finds them from the module's own folder, the C++ build's
// views of them are these.
static std::wstring ExeFolder(){wchar_t exe[32768]{};GetModuleFileNameW(nullptr,exe,32768);return std::filesystem::path(exe).parent_path().wstring();}
#if !CVR_RUST_VRIK
namespace cvr::paths {
const std::wstring& Dir(Folder folder) {
    static std::wstring dirs[9];
    static const wchar_t* names[9]{L"",L"shaders",L"grips",L"archive",L"config",L"log",L"scripts",L"redscript",L"lua"};
    auto& d=dirs[size_t(folder)];
    if(d.empty()){d=ExeFolder()+L"\\";if(*names[size_t(folder)])d+=std::wstring(names[size_t(folder)])+L"\\";}
    return d;
}
std::string DirA(Folder folder){std::string a;for(wchar_t ch:Dir(folder))a.push_back(char(ch));return a;}
}
#endif
namespace {
int liveTwins=0;
void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
class FakePso final:public ID3D12PipelineState {
    std::atomic<ULONG> refs{1};
    bool allocated, twin;
    GUID key{};
    IUnknown* owner{};
public:
    bool rejectPrivateData=false;
    FakePso(bool heap=false,bool replacement=false):allocated(heap),twin(replacement){if(twin)++liveTwins;}
    ~FakePso(){if(owner)owner->Release();if(twin)--liveTwins;}
    ULONG References()const{return refs;}
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{
        if(!out)return E_POINTER;*out=nullptr;
        if(id!=__uuidof(IUnknown)&&id!=__uuidof(ID3D12Object)&&id!=__uuidof(ID3D12DeviceChild)&&id!=__uuidof(ID3D12PipelineState))return E_NOINTERFACE;
        *out=static_cast<ID3D12PipelineState*>(this);AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto count=--refs;if(!count){if(allocated)delete this;else this->~FakePso();}return count;}
    HRESULT STDMETHODCALLTYPE GetPrivateData(REFGUID guid,UINT* bytes,void* data)override{
        if(!bytes)return E_POINTER;if(!owner||guid!=key)return DXGI_ERROR_NOT_FOUND;
        if(!data||*bytes<sizeof(owner)){*bytes=sizeof(owner);return DXGI_ERROR_MORE_DATA;}
        owner->AddRef();*static_cast<IUnknown**>(data)=owner;*bytes=sizeof(owner);return S_OK;
    }
    HRESULT STDMETHODCALLTYPE SetPrivateData(REFGUID,UINT,const void*)override{return E_NOTIMPL;}
    HRESULT STDMETHODCALLTYPE SetPrivateDataInterface(REFGUID guid,const IUnknown* data)override{
        if(rejectPrivateData)return E_FAIL;
        auto* next=const_cast<IUnknown*>(data);if(next)next->AddRef();
        auto* previous=owner;owner=next;key=guid;if(previous)previous->Release();return S_OK;
    }
    HRESULT STDMETHODCALLTYPE SetName(LPCWSTR)override{return S_OK;}
    HRESULT STDMETHODCALLTYPE GetDevice(REFIID,void** out)override{if(out)*out=nullptr;return E_NOINTERFACE;}
    HRESULT STDMETHODCALLTYPE GetCachedBlob(ID3DBlob** out)override{if(out)*out=nullptr;return E_NOTIMPL;}
};
uint64_t Hash(const uint8_t* bytes,size_t count){uint64_t h=1469598103934665603ull;for(size_t i=0;i<count;++i){h^=bytes[i];h*=1099511628211ull;}return h;}
}
int main(int argc,char** argv){
    try{
        const bool trace=argc>1 && std::strcmp(argv[1],"trace")==0;
        const char* fixture=argc>1 && !trace?argv[1]:CVR_FIXTURE;
        namespace s=cvr::hidden_area::shaders;
        // The census records from the first call on: its switch file is in the log folder.
        std::filesystem::create_directories(std::filesystem::path(ExeFolder())/L"log");
        std::ofstream(std::filesystem::path(ExeFolder())/L"log"/L"cvr_shader_census.txt").put('1');
        const uint8_t patchedBytes[]{1,2,3,4},nativeBytes[]{5,6,7,8};
        const D3D12_SHADER_BYTECODE patched{patchedBytes,4},native{nativeBytes,4};
        wchar_t exe[32768]{};Require(GetModuleFileNameW(nullptr,exe,32768)!=0,"Executable path");
        auto folder=std::filesystem::path(exe).parent_path()/L"shaders"/L"view_dir";std::filesystem::create_directories(folder);
        char name[32];sprintf_s(name,"%016llx.dxil",static_cast<unsigned long long>(Hash(patchedBytes,4)));
        std::filesystem::copy_file(fixture,folder/name,std::filesystem::copy_options::overwrite_existing);
        Require(s::HasPatch(patched)&&!s::HasPatch(native),"Fixture mapping");
        CyberpunkVR_ViewDirPatch=1;
        alignas(FakePso) std::byte storage[sizeof(FakePso)];
        auto* original=new(storage) FakePso;
        ID3D12PipelineState* twin{};
        unsigned creates=0;
        auto creator=[&](const D3D12_SHADER_BYTECODE&){++creates;twin=new FakePso(true,true);return twin;};
        s::PipelineCreated(original,{},patched,{});
        s::Register(original,patched,creator);
        Require(original->References()==1,"Registration must not retain original PSO");
        Require(s::Bind(original,0)==twin&&s::Bind(original,1)==twin,"Both eyes select current twin");
        s::Register(original,patched,creator);
        Require(creates==1&&liveTwins==1,"Repeated registration must preserve original owner/twin");
        Require(original->Release()==0,"Original destroyed");
        Require(liveTwins==0,"Destroying original must release its twin");

        auto* reused=new(storage) FakePso;
        s::PipelineCreated(reused,{},native,{});
        Require(!s::HasPatch(native),"Unpatched pipeline skips Register in device hook");
        Require(s::Bind(reused,0)==reused&&s::Bind(reused,1)==reused,"Reused address must not inherit another PSO's twin");
        reused->Release();

        reused=new(storage) FakePso;
        // A creation path that does not notify PipelineCreated is also a different lifetime.
        Require(s::Bind(reused,0)==reused,"No dependence on creation notification");
        s::Register(reused,patched,creator);
        Require(liveTwins==1&&creates==2&&s::Bind(reused,0)==twin,"New patched lifetime gets its own owner");
        CyberpunkVR_ViewDirPatch=0;
        Require(s::Bind(reused,0)==reused,"Disabled group retains original");
        CyberpunkVR_ViewDirPatch=1;
        reused->Release();
        Require(liveTwins==0,"Second twin released");

        reused=new(storage) FakePso;reused->rejectPrivateData=true;
        s::Register(reused,patched,creator);
        Require(creates==2&&s::Bind(reused,0)==reused,"Failed attachment must retain original");
        reused->Release();
        reused=new(storage) FakePso;
        s::Register(reused,patched,[](const auto&)->ID3D12PipelineState*{return nullptr;});
        Require(s::Bind(reused,0)==reused,"Failed twin creation must retain original");
        reused->Release();
        Require(liveTwins==0,"No live replacement objects remain");

        // The census: dispatches and full-screen draws by compute/pixel shader (mode 1), instanced draws by vertex
        // shader (2), every draw by vertex (3) or pixel shader (4), each shader dumped once.
        auto* counted=new(storage) FakePso;
        s::PipelineCreated(counted,{},patched,native);
        const auto record=[&](uint32_t mode){
            CyberpunkVR_ShaderCensus=mode;
            s::Command(7,0,counted,s::Kind::Dispatch,8,4,1);s::Command(7,0,counted,s::Kind::Dispatch,8,4,1);
            s::Command(7,1,counted,s::Kind::Draw,3,1,0);s::Command(7,1,counted,s::Kind::Draw,300,1,0);
            s::Command(9,0,counted,s::Kind::DrawIndexed,36,10,0);s::Command(9,1,counted,s::Kind::Draw,6,10,0);
            s::Command(11,0,nullptr,s::Kind::DispatchRays,1920,1080,1);
        };
        for(uint32_t mode=0;mode<=4;++mode)record(mode);
        CyberpunkVR_ShaderCensus=0;
        counted->Release();
        unsigned rows=0;uint64_t census=1469598103934665603ull;
        for(auto row:CyberpunkVR_ShaderCensusLog) {
            if(!row.count)break;
            ++rows;
            // Addresses differ between runs: the pipeline is named by what it is.
            row.pso=row.pso==reinterpret_cast<uint64_t>(counted)?1:row.pso?2:0;
            const auto* bytes=reinterpret_cast<const uint8_t*>(&row);
            for(size_t i=0;i<sizeof(row);++i){census^=bytes[i];census*=1099511628211ull;}
        }
        const auto& first=CyberpunkVR_ShaderCensusLog[0];
        // Mode 1: the dispatch (twice), the 3-vertex draw, the ray dispatch; 2: both instanced draws; 3 and 4: three draw rows each.
        if(rows!=11)std::printf("census rows %u\n",rows);
        Require(CyberpunkVR_ShaderCensusRecording==1 && rows==11,"Census rows");
        Require(first.node==7 && first.kind==1 && first.count==2 && first.hash==Hash(patchedBytes,4) && first.bytes==4,"Census dispatch row");
        Require(std::filesystem::exists(std::filesystem::path(ExeFolder())/L"log"/L"cvr_shaders"/std::filesystem::path(name)),
                "Census shader dump");
        if(trace) {
            std::printf("pipeline_lifetime trace registered=%u loaded=%u built=%u failed=%u binds=%llu census=%016llx rows=%u "
                        "pipelines=%llu creates=%u\n",CyberpunkVR_HiddenAreaPatchRegistered.load(),CyberpunkVR_ViewDirPatchLoaded.load(),
                        CyberpunkVR_ViewDirPatchBuilt.load(),CyberpunkVR_ViewDirPatchFailed.load(),
                        static_cast<unsigned long long>(CyberpunkVR_ViewDirPatchBinds.load()),static_cast<unsigned long long>(census),rows,
                        static_cast<unsigned long long>(CyberpunkVR_ShaderCensusPipelines.load()),creates);
            return 0;
        }
        puts("PASS: PSO address reuse, repeated registration, owner destruction and failure fallbacks, shader census");
        return 0;
    }catch(const std::exception& e){printf("FAIL: %s\n",e.what());return 1;}
}
