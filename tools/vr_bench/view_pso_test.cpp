// view_pso_test -- creates single-pass pipelines (src/Render/SinglePassPipelines.cpp) offline with the D3D12 debug
// layer on, so a refused pipeline comes with the runtime's reason. The root signatures are the engine's material root
// (36 tables, as the pair probe logged it) and its twin (+ b0..b3 in space 10 for each stage); the input layout and
// render targets are made from each shader's own signatures.
//
//   view_pso_test.exe <dxc folder> <original shaders> <twin shaders> [<vs hash> [<ps hash>]]
// Without hashes: every vertex shader alone (depth-only), then every pixel shader with a vertex shader whose
// outputs match its inputs.
#include "../../src/Render/ViewTwinIr.cpp"
#include "../../src/Render/ViewTwinCompiler.cpp"
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include "Render/ViewInstancedPipeline.hpp"
#include <dxgi1_6.h>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <map>
#include <regex>
#pragma comment(lib,"d3d12.lib")
#pragma comment(lib,"dxgi.lib")
#pragma comment(lib,"dxguid.lib")
namespace fs=std::filesystem;

// With D3D12\D3D12Core.dll and D3D12\d3d12SDKLayers.dll of the Agility SDK 1.614 beside the exe, the debug layer is
// there even without the Graphics Tools feature (the runtime's messages for a refused pipeline).
extern "C" {__declspec(dllexport) extern const UINT D3D12SDKVersion=614;}
extern "C" {__declspec(dllexport) extern const char* D3D12SDKPath=".\\D3D12\\";}
static ComPtr<ID3D12Device> device;
static ComPtr<ID3D12InfoQueue> info;
static void Messages() {
    if(!info)return;
    for(UINT64 i=0;i<info->GetNumStoredMessages();++i) {
        SIZE_T n=0;info->GetMessage(i,nullptr,&n);std::vector<char> b(n);auto* m=reinterpret_cast<D3D12_MESSAGE*>(b.data());
        if(SUCCEEDED(info->GetMessage(i,m,&n)))std::printf("    D3D12: %.*s\n",int(m->DescriptionByteLength),m->pDescription);
    }
    info->ClearStoredMessages();
}
static ComPtr<ID3D12RootSignature> Root(bool twin) {
    struct R{D3D12_DESCRIPTOR_RANGE_TYPE t;UINT base,count,space;};
    const R stage[18]{{D3D12_DESCRIPTOR_RANGE_TYPE_CBV,0,4,0},{D3D12_DESCRIPTOR_RANGE_TYPE_CBV,4,1,0},{D3D12_DESCRIPTOR_RANGE_TYPE_CBV,5,2,0},
        {D3D12_DESCRIPTOR_RANGE_TYPE_CBV,7,1,0},{D3D12_DESCRIPTOR_RANGE_TYPE_CBV,8,6,0},{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,0,8,0},
        {D3D12_DESCRIPTOR_RANGE_TYPE_SRV,8,24,0},{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,32,22,0},{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,54,39,0},
        {D3D12_DESCRIPTOR_RANGE_TYPE_SRV,93,34,0},{D3D12_DESCRIPTOR_RANGE_TYPE_UAV,0,19,0},{D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,0,8,0},
        {D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER,8,8,0},{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,0,UINT_MAX,1},{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,0,UINT_MAX,2},
        {D3D12_DESCRIPTOR_RANGE_TYPE_SRV,0,UINT_MAX,3},{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,0,UINT_MAX,4},{D3D12_DESCRIPTOR_RANGE_TYPE_SRV,0,UINT_MAX,5}};
    static D3D12_DESCRIPTOR_RANGE1 ranges[38];D3D12_ROOT_PARAMETER1 params[38]{};
    const UINT n=twin?38:36;
    for(UINT i=0;i<n;++i) {
        const R r=i<36?stage[i%18]:R{D3D12_DESCRIPTOR_RANGE_TYPE_CBV,0,4,10};
        const bool sampler=r.t==D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER;
        ranges[i]={r.t,r.count,r.base,r.space,sampler?D3D12_DESCRIPTOR_RANGE_FLAG_DESCRIPTORS_VOLATILE:
                   D3D12_DESCRIPTOR_RANGE_FLAG_DESCRIPTORS_VOLATILE|D3D12_DESCRIPTOR_RANGE_FLAG_DATA_VOLATILE,D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND};
        params[i].ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;params[i].DescriptorTable={1,&ranges[i]};
        params[i].ShaderVisibility=(i<36?i<18:i==36)?D3D12_SHADER_VISIBILITY_VERTEX:D3D12_SHADER_VISIBILITY_PIXEL;
    }
    D3D12_VERSIONED_ROOT_SIGNATURE_DESC d{D3D_ROOT_SIGNATURE_VERSION_1_1};
    d.Desc_1_1={n,params,0,nullptr,D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT};
    ComPtr<ID3DBlob> blob,err;
    if(FAILED(D3D12SerializeVersionedRootSignature(&d,&blob,&err))){std::printf("root serialize failed\n");return nullptr;}
    ComPtr<ID3D12RootSignature> root;
    if(FAILED(device->CreateRootSignature(0,blob->GetBufferPointer(),blob->GetBufferSize(),IID_PPV_ARGS(&root)))){Messages();return nullptr;}
    return root;
}
struct Sig {std::string name;int index;std::string mask,sysValue,format;};
static std::vector<Sig> Signature(const std::string& text,const char* which) {
    std::vector<Sig> out;const auto at=text.find(which);if(at==std::string::npos)return out;
    std::istringstream in(text.substr(at));std::string line;
    while(std::getline(in,line) && line.find("; ----")==std::string::npos){}
    while(std::getline(in,line) && line.size()>3 && line[0]==';' && line[2]!=' ' && line[2]!='\r') {
        std::istringstream f(line.substr(1));Sig s;std::string reg,used;
        f>>s.name>>s.index>>s.mask>>reg>>s.sysValue>>s.format;if(!f)break;out.push_back(s);
    }
    return out;
}
static std::string Disassemble(const std::vector<uint8_t>& b) {
    static ComPtr<IDxcCompiler> c;static ComPtr<IDxcUtils> u;
    if(!c){auto create=reinterpret_cast<DxcCreateInstanceProc>(GetProcAddress(GetModuleHandleW(L"dxcompiler.dll"),"DxcCreateInstance"));
           create(CLSID_DxcCompiler,IID_PPV_ARGS(&c));create(CLSID_DxcUtils,IID_PPV_ARGS(&u));}
    ComPtr<IDxcBlobEncoding> blob,text;u->CreateBlob(b.data(),UINT32(b.size()),DXC_CP_ACP,&blob);c->Disassemble(blob.Get(),&text);
    return text?std::string(static_cast<const char*>(text->GetBufferPointer()),text->GetBufferSize()):"";
}
static std::vector<uint8_t> Load(const fs::path& p){std::ifstream f(p,std::ios::binary);return {std::istreambuf_iterator<char>(f),{}};}
static DXGI_FORMAT Format(const Sig& s) {
    const int n=int(s.mask.size());
    const bool i=s.format=="int",u=s.format=="uint";
    const DXGI_FORMAT f[4]={i?DXGI_FORMAT_R32_SINT:u?DXGI_FORMAT_R32_UINT:DXGI_FORMAT_R32_FLOAT,i?DXGI_FORMAT_R32G32_SINT:u?DXGI_FORMAT_R32G32_UINT:DXGI_FORMAT_R32G32_FLOAT,
        i?DXGI_FORMAT_R32G32B32_SINT:u?DXGI_FORMAT_R32G32B32_UINT:DXGI_FORMAT_R32G32B32_FLOAT,i?DXGI_FORMAT_R32G32B32A32_SINT:u?DXGI_FORMAT_R32G32B32A32_UINT:DXGI_FORMAT_R32G32B32A32_FLOAT};
    return f[std::clamp(n,1,4)-1];
}
static HRESULT Create(const std::vector<uint8_t>& vs,const std::vector<uint8_t>* ps,ID3D12RootSignature* root,bool views,const char* what) {
    const auto inputs=Signature(Disassemble(vs),"; Input signature:");
    std::vector<D3D12_INPUT_ELEMENT_DESC> layout;
    for(const auto& s:inputs)if(s.sysValue=="NONE")layout.push_back({s.name.c_str(),UINT(s.index),Format(s),0,D3D12_APPEND_ALIGNED_ELEMENT,
                                                                     D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA,0});
    D3D12_GRAPHICS_PIPELINE_STATE_DESC d{};
    d.pRootSignature=root;d.VS={vs.data(),vs.size()};if(ps)d.PS={ps->data(),ps->size()};
    d.BlendState.RenderTarget[0].RenderTargetWriteMask=0xF;for(auto& t:d.BlendState.RenderTarget)t.RenderTargetWriteMask=0xF;
    d.SampleMask=~0u;d.RasterizerState.FillMode=D3D12_FILL_MODE_SOLID;d.RasterizerState.CullMode=D3D12_CULL_MODE_BACK;d.RasterizerState.DepthClipEnable=TRUE;
    d.DepthStencilState.DepthEnable=TRUE;d.DepthStencilState.DepthWriteMask=D3D12_DEPTH_WRITE_MASK_ALL;d.DepthStencilState.DepthFunc=D3D12_COMPARISON_FUNC_GREATER;
    d.InputLayout={layout.data(),UINT(layout.size())};d.PrimitiveTopologyType=D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;
    if(ps) {
        const auto outputs=Signature(Disassemble(*ps),"; Output signature:");
        for(const auto& s:outputs)if(s.name=="SV_Target")d.NumRenderTargets=std::max<UINT>(d.NumRenderTargets,UINT(s.index)+1);
        for(UINT i=0;i<d.NumRenderTargets;++i)d.RTVFormats[i]=DXGI_FORMAT_R16G16B16A16_FLOAT;
    }
    d.DSVFormat=DXGI_FORMAT_D32_FLOAT_S8X24_UINT;d.SampleDesc={1,0};
    D3D12_VIEW_INSTANCE_LOCATION loc[2]{{0,0},{1,0}};
    struct alignas(void*) Part {D3D12_PIPELINE_STATE_SUBOBJECT_TYPE type;D3D12_VIEW_INSTANCING_DESC value;};
    HRESULT hr;ComPtr<ID3D12PipelineState> pso;
    if(views) {
        ComPtr<ID3D12Device2> d2;device.As(&d2);
        std::vector<uint8_t> stream;
        if(!cvr::stereo::GraphicsDescStream(d,stream)){std::printf("  %s: stream failed\n",what);return E_FAIL;}
        Part p{D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VIEW_INSTANCING,{2,loc,D3D12_VIEW_INSTANCING_FLAG_NONE}};
        const auto* b=reinterpret_cast<const uint8_t*>(&p);stream.insert(stream.end(),b,b+sizeof(p));
        const D3D12_PIPELINE_STATE_STREAM_DESC sd{stream.size(),stream.data()};
        hr=d2->CreatePipelineState(&sd,IID_PPV_ARGS(&pso));
    }else hr=device->CreateGraphicsPipelineState(&d,IID_PPV_ARGS(&pso));
    std::printf("  %s: %08X\n",what,unsigned(hr));Messages();
    return hr;
}
int wmain(int argc,wchar_t** argv) {
    if(argc<4){std::printf("view_pso_test <dxc> <originals> <twins> [vs [ps]]\n");return 2;}
    cvr::single_pass::ViewTwinCompiler compiler;std::string error;
    if(!compiler.Load(argv[1],error)){std::printf("%s\n",error.c_str());return 2;}
    // The Agility SDK beside the exe through a device factory (no developer mode needed), else the system's layer.
    ComPtr<ID3D12SDKConfiguration1> config;ComPtr<ID3D12DeviceFactory> factory;
    HRESULT dh=D3D12GetInterface(CLSID_D3D12SDKConfiguration,IID_PPV_ARGS(&config));
    if(SUCCEEDED(dh))dh=config->CreateDeviceFactory(D3D12SDKVersion,D3D12SDKPath,IID_PPV_ARGS(&factory));
    if(factory) {
        ComPtr<ID3D12Debug> debug;dh=factory->GetConfigurationInterface(CLSID_D3D12Debug,IID_PPV_ARGS(&debug));
        if(SUCCEEDED(dh))debug->EnableDebugLayer();
        if(FAILED(factory->CreateDevice(nullptr,D3D_FEATURE_LEVEL_12_0,IID_PPV_ARGS(&device))))return 2;
    } else {
        ComPtr<ID3D12Debug> debug;dh=D3D12GetDebugInterface(IID_PPV_ARGS(&debug));if(SUCCEEDED(dh))debug->EnableDebugLayer();
        if(FAILED(D3D12CreateDevice(nullptr,D3D_FEATURE_LEVEL_12_0,IID_PPV_ARGS(&device))))return 2;
    }
    std::printf("debug layer: %08X (factory %p)\n",unsigned(dh),static_cast<void*>(factory.Get()));
    device.As(&info);std::printf("info queue: %p\n",static_cast<void*>(info.Get()));
    const auto root=Root(false),twinRoot=Root(true);
    if(!root || !twinRoot)return 2;
    const fs::path originals(argv[2]),twins(argv[3]);
    const auto twinOf=[&](const fs::path& original){const auto t=twins/original.filename();return fs::exists(t)?Load(t):Load(original);};
    if(argc>4 && std::wstring(argv[4])==L"ps") {   // every pixel shader with a vertex shader whose outputs feed it
        struct Shader {fs::path path;std::vector<uint8_t> bytes;std::vector<Sig> in,out;bool vs;};
        std::vector<Shader> all;
        for(const auto& e:fs::directory_iterator(originals)) {
            if(e.path().extension()!=L".dxil")continue;
            Shader s{e.path(),Load(e.path())};const auto t=Disassemble(s.bytes);
            if(t.find("!\"vs\"")!=std::string::npos)s.vs=true;else if(t.find("!\"ps\"")==std::string::npos)continue;
            s.in=Signature(t,"; Input signature:");s.out=Signature(t,"; Output signature:");all.push_back(std::move(s));
        }
        const auto key=[](const std::vector<Sig>& v,bool skipSystem){std::string k;
            for(const auto& s:v)if(!skipSystem || s.sysValue=="NONE")k+=s.name+std::to_string(s.index)+",";return k;};
        int failed=0,tested=0,unmatched=0;
        for(const auto& ps:all) {
            if(ps.vs)continue;
            const Shader* vs{};
            for(const auto& v:all)   // a vertex shader the original pixel shader links with
                if(v.vs && key(v.out,true)==key(ps.in,true) && SUCCEEDED(Create(v.bytes,&ps.bytes,root.Get(),false,"original VS + PS"))){vs=&v;break;}
            if(!vs){++unmatched;continue;}
            ++tested;std::printf("%ls with %ls\n",ps.path.filename().c_str(),vs->path.filename().c_str());
            const auto psTwin=twinOf(ps.path),vsTwin=twinOf(vs->path);
            if(FAILED(Create(vsTwin,&psTwin,twinRoot.Get(),true,"twins + twin root + 2 views"))) {
                ++failed;
                Create(vs->bytes,&ps.bytes,twinRoot.Get(),true,"  original shaders + twin root + 2 views");
                Create(vsTwin,&ps.bytes,twinRoot.Get(),true,"  twin VS, original PS + 2 views");
                Create(vs->bytes,&psTwin,twinRoot.Get(),true,"  original VS, twin PS + 2 views");
                Create(vsTwin,&psTwin,twinRoot.Get(),false,"  twins, no views");
            }
        }
        std::printf("%d of %d failed, %d pixel shaders without a matching vertex shader\n",failed,tested,unmatched);
        return failed?1:0;
    }
    std::vector<fs::path> list;
    if(argc>4)list.push_back(originals/(std::wstring(argv[4])+L".dxil"));
    else for(const auto& e:fs::directory_iterator(originals))if(e.path().extension()==L".dxil")list.push_back(e.path());
    int failed=0,tested=0;
    for(const auto& path:list) {
        const auto bytes=Load(path);const auto text=Disassemble(bytes);
        if(text.find("; Vertex Shader")==std::string::npos && text.find("!\"vs\"")==std::string::npos)continue;
        if(++tested>12 && argc<=4)break;
        std::printf("%ls\n",path.filename().c_str());
        Create(bytes,nullptr,root.Get(),false,"original VS, depth only");
        const auto twin=twinOf(path);
        if(FAILED(Create(twin,nullptr,twinRoot.Get(),true,"twin VS + twin root + 2 views")))++failed;
        if(argc>5) {
            const auto ps=Load(originals/(std::wstring(argv[5])+L".dxil"));const auto psTwin=twinOf(originals/(std::wstring(argv[5])+L".dxil"));
            Create(bytes,&ps,root.Get(),false,"original VS + PS");
            if(FAILED(Create(twin,&psTwin,twinRoot.Get(),true,"twins + twin root + 2 views")))++failed;
            Create(bytes,&ps,twinRoot.Get(),true,"original shaders + twin root + 2 views");
            // the twins as the plugin builds them: the pixel shader's for this vertex shader
            const auto vsBuilt=compiler.Build(bytes.data(),bytes.size());
            const auto psBuilt=compiler.Build(ps.data(),ps.size(),bytes.data(),bytes.size());
            std::printf("  built: vs %s%s, ps %s%s%s\n",vsBuilt.ok?"ok":vsBuilt.error.c_str(),vsBuilt.unchanged?" (unchanged)":"",
                        psBuilt.ok?"ok":psBuilt.error.c_str(),psBuilt.unchanged?" (unchanged)":"",psBuilt.linked?" (inputs completed)":"");
            if(vsBuilt.ok && psBuilt.ok) {
                const auto v=vsBuilt.unchanged?bytes:vsBuilt.container;const auto f=psBuilt.unchanged?ps:psBuilt.container;
                if(FAILED(Create(v,&f,twinRoot.Get(),true,"plugin twins + twin root + 2 views")))++failed;
            }
        }
    }
    std::printf("%d of %d failed\n",failed,tested);
    return failed?1:0;
}
