// In-place DXIL constant patches on the real adapter (src/Render/DxilPatch.cpp).
//
// A compute shader with two placeholder constants is compiled by the Windows SDK's dxcompiler, its
// container hash is checked against our implementation, both placeholders are found exactly once and
// rewritten, the container is re-signed, and the device runs it: out = in * A + B must use the new
// values. That proves the driver accepts the rewritten bitcode and that the constants took effect.
//   hidden_area_dxil_patch [DXC directory]
//   hidden_area_dxil_patch trace   -- the same calls on synthetic containers (every hash length and padding branch,
//                                     truncated and malformed headers, placeholders found, rewritten, re-signed),
//                                     hashed, so the C++ and the Rust build compare bit for bit without dxc or a GPU
//                                     (cmake/ParityTrace.cmake)
#include "Render/DxilPatch.hpp"
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <dxcapi.h>
#include <wrl/client.h>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

using Microsoft::WRL::ComPtr;
using namespace cvr::dxil_patch;
static int failures=0;
#define CHECK(x) do{if(!(x)){std::printf("FAIL %s:%d %s\n",__FILE__,__LINE__,#x);++failures;}}while(0)

static const char* Source=R"hlsl(
#define RS "RootFlags(0), UAV(u0)"
RWStructuredBuffer<float> data : register(u0);
[RootSignature(RS)] [numthreads(1,1,1)] void main() { data[1] = data[0] * 1.2345678f + 1.3456789f; }
)hlsl";

static std::vector<uint8_t> Compile(const std::wstring& dxcDirectory) {
    const auto path=dxcDirectory+L"\\dxcompiler.dll";
    HMODULE module=LoadLibraryExW(path.c_str(),nullptr,LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    auto create=module?reinterpret_cast<DxcCreateInstanceProc>(GetProcAddress(module,"DxcCreateInstance")):nullptr;
    if(!create){std::printf("dxcompiler not found in %ls\n",dxcDirectory.c_str());return {};}
    ComPtr<IDxcCompiler3> compiler;ComPtr<IDxcResult> result;ComPtr<IDxcBlob> object;
    if(FAILED(create(CLSID_DxcCompiler,IID_PPV_ARGS(&compiler))))return {};
    const DxcBuffer buffer{Source,std::strlen(Source),DXC_CP_UTF8};
    LPCWSTR args[]{L"-T",L"cs_6_0",L"-E",L"main"};
    if(FAILED(compiler->Compile(&buffer,args,4,nullptr,IID_PPV_ARGS(&result))))return {};
    HRESULT status{};result->GetStatus(&status);
    if(FAILED(status) || FAILED(result->GetOutput(DXC_OUT_OBJECT,IID_PPV_ARGS(&object),nullptr)) || !object)return {};
    const auto* bytes=static_cast<const uint8_t*>(object->GetBufferPointer());
    return {bytes,bytes+object->GetBufferSize()};
}

static bool Run(ID3D12Device* device,const std::vector<uint8_t>& shader,float input,float& output) {
    ComPtr<ID3D12RootSignature> root;ComPtr<ID3D12PipelineState> pso;
    if(FAILED(device->CreateRootSignature(0,shader.data(),shader.size(),IID_PPV_ARGS(&root))))return false;
    D3D12_COMPUTE_PIPELINE_STATE_DESC desc{};desc.pRootSignature=root.Get();desc.CS={shader.data(),shader.size()};
    if(FAILED(device->CreateComputePipelineState(&desc,IID_PPV_ARGS(&pso))))return false;
    D3D12_HEAP_PROPERTIES upload{D3D12_HEAP_TYPE_UPLOAD},readback{D3D12_HEAP_TYPE_READBACK},local{D3D12_HEAP_TYPE_DEFAULT};
    D3D12_RESOURCE_DESC buffer{D3D12_RESOURCE_DIMENSION_BUFFER,0,256,1,1,1,DXGI_FORMAT_UNKNOWN,{1,0},D3D12_TEXTURE_LAYOUT_ROW_MAJOR};
    ComPtr<ID3D12Resource> source,data,result;
    if(FAILED(device->CreateCommittedResource(&upload,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_GENERIC_READ,nullptr,IID_PPV_ARGS(&source))) ||
       FAILED(device->CreateCommittedResource(&readback,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&result))))return false;
    buffer.Flags=D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    if(FAILED(device->CreateCommittedResource(&local,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_COMMON,nullptr,IID_PPV_ARGS(&data))))return false;
    void* mapped{};source->Map(0,nullptr,&mapped);float values[2]{input,0};std::memcpy(mapped,values,8);source->Unmap(0,nullptr);
    D3D12_COMMAND_QUEUE_DESC qd{};ComPtr<ID3D12CommandQueue> queue;ComPtr<ID3D12CommandAllocator> allocator;ComPtr<ID3D12GraphicsCommandList> list;
    if(FAILED(device->CreateCommandQueue(&qd,IID_PPV_ARGS(&queue))) ||
       FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator))) ||
       FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),pso.Get(),IID_PPV_ARGS(&list))))return false;
    list->CopyBufferRegion(data.Get(),0,source.Get(),0,256);
    D3D12_RESOURCE_BARRIER b{};b.Transition={data.Get(),0,D3D12_RESOURCE_STATE_COPY_DEST,D3D12_RESOURCE_STATE_UNORDERED_ACCESS};list->ResourceBarrier(1,&b);
    list->SetComputeRootSignature(root.Get());list->SetComputeRootUnorderedAccessView(0,data->GetGPUVirtualAddress());list->Dispatch(1,1,1);
    b.Transition={data.Get(),0,D3D12_RESOURCE_STATE_UNORDERED_ACCESS,D3D12_RESOURCE_STATE_COPY_SOURCE};list->ResourceBarrier(1,&b);
    list->CopyBufferRegion(result.Get(),0,data.Get(),0,256);
    if(FAILED(list->Close()))return false;
    ID3D12CommandList* lists[]{list.Get()};queue->ExecuteCommandLists(1,lists);
    ComPtr<ID3D12Fence> fence;device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence));
    const HANDLE event=CreateEventW(nullptr,FALSE,FALSE,nullptr);queue->Signal(fence.Get(),1);fence->SetEventOnCompletion(1,event);
    if(WaitForSingleObject(event,5000)!=WAIT_OBJECT_0)return false;
    result->Map(0,nullptr,&mapped);std::memcpy(values,mapped,8);result->Unmap(0,nullptr);
    output=values[1];return true;
}

static int Trace() {
    uint64_t h=1469598103934665603ull;
    const auto mix=[&](const void* p,size_t n){for(size_t i=0;i<n;++i){h^=static_cast<const uint8_t*>(p)[i];h*=1099511628211ull;}};
    const auto mixv=[&](uint64_t v){mix(&v,8);};
    uint32_t seed=12345;
    const auto next=[&]{seed=seed*1664525u+1013904223u;return uint8_t(seed>>24);};
    std::vector<uint8_t> noise(400);for(auto& b:noise)b=next();
    for(size_t n=0;n<=300;++n){const auto x=ContainerHash(noise.data(),n);mix(x.data(),16);}
    {const auto x=ContainerHash(nullptr,100);mix(x.data(),16);}
    // Header, an SFI0 part and a DXIL part whose program header points at 160 bytes of bitcode (88..248).
    std::vector<uint8_t> c(248);
    const auto put=[](std::vector<uint8_t>& v,size_t at,uint32_t x){std::memcpy(v.data()+at,&x,4);};
    std::memcpy(c.data(),"DXBC",4);put(c,20,1);put(c,24,248);put(c,28,2);put(c,32,40);put(c,36,56);
    std::memcpy(c.data()+40,"SFI0",4);put(c,44,8);
    std::memcpy(c.data()+56,"DXIL",4);put(c,60,184);
    put(c,64,0x60000);put(c,68,46);std::memcpy(c.data()+72,"DXIL",4);put(c,76,0x106);put(c,80,16);put(c,84,160);
    for(size_t i=88;i<c.size();++i)c[i]=next();
    for(size_t bit:{size_t(88*8+3),size_t(88*8+77),size_t(88*8+700)})mixv(WriteFloat(c,bit,1.2345678f));
    mixv(WriteFloat(c,88*8+5,2.5f));mixv(WriteFloat(c,248*8-30,1.5f));mixv(WriteFloat(c,248*8-36,1.5f));
    {size_t s=0,l=0;mixv(BitcodeRange(c.data(),c.size(),s,l));mixv(s);mixv(l);}
    for(float f:{1.2345678f,1.5f,1.0f,1.9999999f}) {
        const auto found=FindFloat(c.data(),c.size(),f);mixv(found.size());for(auto b:found)mixv(b);
    }
    for(float f:{0.0f,-1.0f,0.99f,1.0f,1.5f,2.0f,std::nanf(""),INFINITY,1e-40f})mixv(PlaceholderValue(f));
    const auto range=[&](const std::vector<uint8_t>& v,size_t n) {
        size_t s=7,l=9;const bool ok=BitcodeRange(v.data(),n,s,l);mixv(ok);mixv(s);mixv(l);
        mixv(FindFloat(v.data(),n,1.2345678f).size());
    };
    for(size_t n=0;n<=c.size();++n)range(c,n);
    std::vector<std::vector<uint8_t>> bad(7,c);
    put(bad[0],28,65);put(bad[1],28,0);put(bad[2],36,300);put(bad[3],60,20);bad[4][72]='X';put(bad[5],84,400);bad[6][0]='X';
    for(const auto& v:bad)range(v,v.size());
    {size_t s=0,l=0;mixv(BitcodeRange(nullptr,100,s,l));}
    auto signedC=c;mixv(Sign(signedC));mix(signedC.data(),signedC.size());
    std::vector<uint8_t> tiny(20,0);mixv(Sign(tiny));
    auto notDxbc=c;notDxbc[0]='X';mixv(Sign(notDxbc));
    std::printf("dxil_patch trace %016llx\n",static_cast<unsigned long long>(h));
    return 0;
}

int wmain(int argc,wchar_t** argv) {
    if(argc>1 && std::wstring(argv[1])==L"trace")return Trace();
    const std::wstring dxc=argc>1?argv[1]:L"C:\\Program Files (x86)\\Windows Kits\\10\\bin\\10.0.26100.0\\x64";
    auto shader=Compile(dxc);
    CHECK(!shader.empty());if(shader.empty())return 1;
    CHECK(ContainerHash(shader.data(),shader.size())==*reinterpret_cast<const Hash*>(shader.data()+4));
    CHECK(PlaceholderValue(1.0f) && PlaceholderValue(1.9999f) && !PlaceholderValue(2.0f) && !PlaceholderValue(0.99f));
    const auto a=FindFloat(shader.data(),shader.size(),1.2345678f), b=FindFloat(shader.data(),shader.size(),1.3456789f);
    CHECK(a.size()==1 && b.size()==1);
    if(a.size()!=1 || b.size()!=1)return 1;
    auto patched=shader;
    CHECK(WriteFloat(patched,a[0],1.5f) && WriteFloat(patched,b[0],1.75f) && !WriteFloat(patched,a[0],2.5f));
    CHECK(FindFloat(patched.data(),patched.size(),1.2345678f).empty() && FindFloat(patched.data(),patched.size(),1.5f).size()==1);
    CHECK(Sign(patched) && ContainerHash(patched.data(),patched.size())==*reinterpret_cast<const Hash*>(patched.data()+4));
    CHECK(patched.size()==shader.size());

    ComPtr<IDXGIFactory6> factory;ComPtr<IDXGIAdapter1> adapter;ComPtr<ID3D12Device> device;
    if(FAILED(CreateDXGIFactory2(0,IID_PPV_ARGS(&factory))) ||
       FAILED(factory->EnumAdapterByGpuPreference(0,DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,IID_PPV_ARGS(&adapter))) ||
       FAILED(D3D12CreateDevice(adapter.Get(),D3D_FEATURE_LEVEL_12_0,IID_PPV_ARGS(&device)))){std::printf("no D3D12 device\n");return 1;}
    float original{},rewritten{},unsigned_{};
    CHECK(Run(device.Get(),shader,2.0f,original) && std::fabs(original-(2.0f*1.2345678f+1.3456789f))<1e-5f);
    CHECK(Run(device.Get(),patched,2.0f,rewritten) && rewritten==4.75f);
    auto stale=patched;std::memcpy(stale.data()+4,shader.data()+4,16);   // rewritten but not re-signed
    CHECK(!Run(device.Get(),stale,2.0f,unsigned_));
    std::printf("original %.7f, rewritten %.7f (expected 4.75); stale hash refused: %s\n",original,rewritten,failures?"?":"yes");
    std::printf(failures?"%d failures\n":"all passed\n",failures);
    return failures?1:0;
}
