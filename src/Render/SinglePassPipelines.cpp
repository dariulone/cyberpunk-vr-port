// SinglePassPipelines -- see include/Render/SinglePassPipelines.hpp.
#include "Render/SinglePassPipelines.hpp"
#include "Render/HiddenAreaShaders.hpp"
#include "Render/ViewInstancedPipeline.hpp"
#include "Render/ViewTwinCompiler.hpp"
#include "Render/ViewTwinIr.hpp"
#include "Stereo/SinglePass.hpp"
#include "Core/Paths.hpp"
#include <windows.h>
#include <wrl/client.h>
#include <chrono>
#include <condition_variable>
#include <cstring>
#include <deque>
#include <filesystem>
#include <fstream>
#include <memory>
#include <mutex>
#include <thread>
#include <unordered_map>
#include <vector>

extern void Log(const char*,...);
using Microsoft::WRL::ComPtr;

extern "C" {
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_SinglePassPipelineCounts[12]{};
__declspec(dllexport) char CyberpunkVR_SinglePassError[256]{};
// 0: no material pipeline is registered (and nothing else of single pass runs); read once, at the first root.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SinglePassPipelines{1};
}

namespace cvr::single_pass {
using cvr::stereo::BuildViewInstancedStream;using cvr::stereo::GraphicsDescStream;using cvr::stereo::OwnedGraphicsDesc;
using cvr::stereo::OwnedPipelineStream;using cvr::stereo::OwnGraphicsDesc;using cvr::stereo::OwnPipelineStream;
using cvr::stereo::PipelineStreamInfo;using cvr::stereo::ReadPipelineStream;using cvr::stereo::ViewInstancedLayout;
namespace {
auto& Count(size_t i){return CyberpunkVR_SinglePassPipelineCounts[i];}
void Error(const char* what) {
    static std::atomic<int> logged{0};
    if(logged.fetch_add(1,std::memory_order_relaxed)<24)Log("[single-pass] %s\n",what);
    strncpy_s(CyberpunkVR_SinglePassError,what,_TRUNCATE);
}
uint64_t Fnv(const void* data,size_t size) {
    uint64_t h=1469598103934665603ull;
    for(size_t i=0;i<size;++i){h^=static_cast<const uint8_t*>(data)[i];h*=1099511628211ull;}
    return h;
}

// ---- roots --------------------------------------------------------------------------------------------------
struct RootInfo {ComPtr<ID3D12RootSignature> twin;};
std::mutex rootMutex;
std::unordered_map<ID3D12RootSignature*,RootInfo> roots;   // material roots only
std::atomic<uint32_t> rootCount{0};

bool TableIsCameraBlock(const D3D12_ROOT_PARAMETER1& p,D3D12_SHADER_VISIBILITY visibility) {
    if(p.ParameterType!=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE || p.ShaderVisibility!=visibility ||
       p.DescriptorTable.NumDescriptorRanges!=1)return false;
    const auto& r=p.DescriptorTable.pDescriptorRanges[0];
    return r.RangeType==D3D12_DESCRIPTOR_RANGE_TYPE_CBV && r.BaseShaderRegister==0 && r.NumDescriptors==4 && r.RegisterSpace==0;
}

// ---- shader twins (memory + disk) -------------------------------------------------------------------------------
struct ShaderTwin {bool ok{},unchanged{};std::vector<uint8_t> bytes;};
std::mutex shaderMutex;
std::unordered_map<uint64_t,std::shared_ptr<const ShaderTwin>> shaders;
std::wstring CacheDir() {
    static const std::wstring dir=[]{
        std::wstring d=cvr::paths::Dir(cvr::paths::Folder::Root)+L"cache\\view_twins\\";
        std::error_code ec;std::filesystem::create_directories(d,ec);
        return d;
    }();
    return dir;
}
std::wstring CacheName(uint64_t hash,const wchar_t* ext) {
    wchar_t name[64];swprintf_s(name,L"%016llx.r%u.%s",static_cast<unsigned long long>(hash),kTwinRecipe,ext);
    return CacheDir()+name;
}
bool ReadFile(const std::wstring& path,std::vector<uint8_t>& out) {
    std::ifstream f(path,std::ios::binary);if(!f)return false;
    out.assign(std::istreambuf_iterator<char>(f),{});return true;
}

// ---- pipelines ---------------------------------------------------------------------------------------------------
constexpr GUID pipelineKey{0x5a1e9b37,0x2c4d,0x4f61,{0x9e,0x0a,0x31,0x7c,0x55,0x8b,0x02,0xd4}};
}

struct Pipeline final:public IUnknown {
    std::atomic<ULONG> refs{1};
    ID3D12Device* device{};
    CreateStreamFn create{};
    ID3D12RootSignature* root{};
    // the owned description: a stream (graphics descriptions are converted at registration)
    OwnedPipelineStream stream;
    OwnedGraphicsDesc graphics;   // keeps the pointed-at parts of a converted graphics description
    std::vector<uint8_t> converted;
    D3D12_PIPELINE_STATE_STREAM_DESC Desc()const {
        return converted.empty()?D3D12_PIPELINE_STATE_STREAM_DESC{stream.stream.size(),const_cast<uint8_t*>(stream.stream.data())}:
                                 D3D12_PIPELINE_STATE_STREAM_DESC{converted.size(),const_cast<uint8_t*>(converted.data())};
    }
    D3D12_SHADER_BYTECODE vs{},ps{};
    std::shared_ptr<const std::vector<uint8_t>> vsBlob,psBlob;   // one copy of each shader for all its pipelines
    uint32_t slots{};   // vertex buffer slots the input layout reads
    // [0] the engine's shaders, [1] their hidden-area replacements (BoundCode): 0 idle, 1 queued, 2 ready, 3 failed
    std::atomic<int> state[2]{};
    // whether a hidden-area twin replaces a stage (BoundCode hashes the bytecode: asked once, not per draw)
    std::atomic<int> replaced{-1};
    ComPtr<ID3D12PipelineState> twin[2];
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override {
        if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;
    }
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{const auto n=--refs;if(!n)delete this;return n;}
};

namespace {
// ---- the worker --------------------------------------------------------------------------------------------------
struct Job {ComPtr<Pipeline> pipeline;int variant;};
std::mutex jobMutex;
std::condition_variable jobSignal;
std::deque<Job> jobs;
std::once_flag workerOnce;
std::atomic<int> compilerState{0};   // 0 untried, 1 ready, 2 unavailable

// A shader's twin from memory, the disk cache or DXC. A pixel shader whose twin depends on its vertex shader (inputs
// completed, ViewTwinIr.hpp) has a marker <hash>.link and its twins per vertex shader <hash>_<vertex hash>.
std::shared_ptr<const ShaderTwin> Load(uint64_t key,const std::wstring& base) {
    auto twin=std::make_shared<ShaderTwin>();std::error_code ec;
    if(std::filesystem::exists(base+L"same",ec)){twin->ok=twin->unchanged=true;}
    else if(std::filesystem::exists(base+L"fail",ec)){}
    else if(ReadFile(base+L"dxil",twin->bytes) && twin->bytes.size()>32){twin->ok=true;}
    else return nullptr;
    Count(8)++;
    (void)key;
    return twin;
}
std::wstring Base(uint64_t hash,uint64_t vertex) {
    wchar_t name[96];
    if(vertex)swprintf_s(name,L"%016llx_%016llx.r%u.",static_cast<unsigned long long>(hash),static_cast<unsigned long long>(vertex),kTwinRecipe);
    else swprintf_s(name,L"%016llx.r%u.",static_cast<unsigned long long>(hash),kTwinRecipe);
    return CacheDir()+name;
}
std::unordered_map<uint64_t,bool> linkedShaders;   // pixel shaders whose twin depends on the vertex shader
std::unordered_map<uint64_t,int> building;         // shaders a worker is building now (the others wait for it)
std::condition_variable built;
std::shared_ptr<const ShaderTwin> BuildShader(ViewTwinCompiler& compiler,const D3D12_SHADER_BYTECODE& code,
                                              const D3D12_SHADER_BYTECODE* vertex=nullptr) {
    const uint64_t hash=Fnv(code.pShaderBytecode,code.BytecodeLength);
    const uint64_t vertexHash=vertex?Fnv(vertex->pShaderBytecode,vertex->BytecodeLength):0;
    const uint64_t pairKey=hash^(vertexHash*0x9E3779B97F4A7C15ull+0x632BE59BD9B4E019ull);
    std::error_code ec;
    bool linked=false;
    {
        std::unique_lock lock(shaderMutex);
        built.wait(lock,[&]{return !building.count(hash);});
        if(const auto l=linkedShaders.find(hash);l!=linkedShaders.end())linked=l->second;
        else linked=linkedShaders[hash]=std::filesystem::exists(Base(hash,0)+L"link",ec);
        const uint64_t key=linked && vertex?pairKey:hash;
        if(const auto it=shaders.find(key);it!=shaders.end())return it->second;
        building[hash]=1;
    }
    struct Done {uint64_t h;~Done(){{std::lock_guard lock(shaderMutex);building.erase(h);}built.notify_all();}} done{hash};
    std::shared_ptr<const ShaderTwin> twin=linked && vertex?Load(pairKey,Base(hash,vertexHash)):Load(hash,Base(hash,0));
    if(!twin) {
        auto built=std::make_shared<ShaderTwin>();
        const auto result=compiler.Build(code.pShaderBytecode,code.BytecodeLength,vertex?vertex->pShaderBytecode:nullptr,
                                         vertex?vertex->BytecodeLength:0);
        if(result.linked && vertex && !linked) {   // from now on per vertex shader
            linked=true;std::ofstream(Base(hash,0)+L"link");
            std::lock_guard lock(shaderMutex);linkedShaders[hash]=true;   // (the lock ends with this block)
        }
        const std::wstring base=Base(hash,linked && vertex?vertexHash:0);
        if(result.ok && result.unchanged){built->ok=built->unchanged=true;Count(6)++;std::ofstream(base+L"same");}
        else if(result.ok) {
            built->ok=true;built->bytes=result.container;Count(5)++;
            std::ofstream(base+L"dxil",std::ios::binary).write(reinterpret_cast<const char*>(built->bytes.data()),
                                                             std::streamsize(built->bytes.size()));
        }else {
            Count(7)++;std::ofstream(base+L"fail")<<result.error;
            char m[200];sprintf_s(m,"shader %016llx: %s",static_cast<unsigned long long>(hash),result.error.substr(0,140).c_str());Error(m);
        }
        twin=built;
    }
    std::lock_guard lock(shaderMutex);
    return shaders.emplace(linked && vertex?pairKey:hash,std::move(twin)).first->second;
}

void BuildPipeline(ViewTwinCompiler& compiler,Pipeline& p,int variant) {
    const auto t0=std::chrono::steady_clock::now();
    const auto vs=variant?cvr::hidden_area::shaders::BoundCode(p.vs):p.vs;
    const auto ps=p.ps.BytecodeLength?(variant?cvr::hidden_area::shaders::BoundCode(p.ps):p.ps):D3D12_SHADER_BYTECODE{};
    const auto vsTwin=BuildShader(compiler,vs);
    const auto psTwin=ps.BytecodeLength?BuildShader(compiler,ps,&vs):nullptr;
    ID3D12RootSignature* twinRoot=TwinRoot(p.root);
    int result=3;
    if(vsTwin->ok && (!psTwin || psTwin->ok) && twinRoot) {
        const D3D12_SHADER_BYTECODE v=vsTwin->unchanged?vs:D3D12_SHADER_BYTECODE{vsTwin->bytes.data(),vsTwin->bytes.size()};
        const D3D12_SHADER_BYTECODE f=!psTwin?D3D12_SHADER_BYTECODE{}:psTwin->unchanged?ps:
                                      D3D12_SHADER_BYTECODE{psTwin->bytes.data(),psTwin->bytes.size()};
        ViewInstancedLayout layout;layout.locations={{{0,0},{1,0}}};   // view 0 -> viewport 0, view 1 -> viewport 1
        std::vector<uint8_t> bytes;
        if(BuildViewInstancedStream(p.Desc(),v,layout,bytes,f,twinRoot)) {
            const D3D12_PIPELINE_STATE_STREAM_DESC desc{bytes.size(),bytes.data()};
            ID3D12PipelineState* created{};
            const HRESULT hr=p.create(p.device,&desc,IID_PPV_ARGS(&created));
            if(SUCCEEDED(hr) && created){p.twin[variant].Attach(created);result=2;}
            else {
                char m[200];sprintf_s(m,"view-instanced pipeline refused: 0x%08X",unsigned(hr));Error(m);
                // Which part is refused: the same pipeline with one change at a time (the first few failures only).
                static std::atomic<int> probes{0};
                if(probes.fetch_add(1,std::memory_order_relaxed)<6) {
                    const auto tryStream=[&](const D3D12_SHADER_BYTECODE& sv,const D3D12_SHADER_BYTECODE& sp,ID3D12RootSignature* r,bool views)->HRESULT {
                        std::vector<uint8_t> b;
                        if(views){if(!BuildViewInstancedStream(p.Desc(),sv,layout,b,sp,r))return E_ABORT;}
                        else {
                            // no view instancing part: replace shaders and root in a copy of the owned stream
                            const auto d=p.Desc();b.assign(static_cast<const uint8_t*>(d.pPipelineStateSubobjectStream),
                                                           static_cast<const uint8_t*>(d.pPipelineStateSubobjectStream)+d.SizeInBytes);
                            std::vector<uint8_t> vi;
                            if(!BuildViewInstancedStream(d,sv,layout,vi,sp,r))return E_ABORT;
                            b.assign(vi.begin(),vi.end()-sizeof(void*)-sizeof(D3D12_VIEW_INSTANCING_DESC));   // drop the trailing views part
                        }
                        const D3D12_PIPELINE_STATE_STREAM_DESC sd{b.size(),b.data()};ID3D12PipelineState* t{};
                        const HRESULT r2=p.create(p.device,&sd,IID_PPV_ARGS(&t));if(t)t->Release();return r2;
                    };
                    const HRESULT original=tryStream(vs,ps,p.root,false),originalRoot2=tryStream(vs,ps,twinRoot,false),
                                  twinsNoViews=tryStream(v,f,twinRoot,false),originalViews=tryStream(vs,ps,twinRoot,true),
                                  twinVsOnly=tryStream(v,ps,twinRoot,true),twinPsOnly=tryStream(vs,f,twinRoot,true);
                    sprintf_s(m,"  probe: original %08X, twin root %08X, twin shaders no views %08X, original shaders + views %08X, "
                              "twin VS only %08X, twin PS only %08X",unsigned(original),unsigned(originalRoot2),unsigned(twinsNoViews),
                              unsigned(originalViews),unsigned(twinVsOnly),unsigned(twinPsOnly));
                    Log("[single-pass] %s\n",m);
                    Log("[single-pass]   desc:%s\n",cvr::stereo::DescribePipelineStream(p.Desc()).c_str());
                    // the views on one viewport for both (is viewport 1 the problem?)
                    ViewInstancedLayout same;same.locations={{{0,0},{0,0}}};
                    std::vector<uint8_t> b1;HRESULT oneViewport=E_ABORT;
                    if(BuildViewInstancedStream(p.Desc(),vs,same,b1,ps,twinRoot)){const D3D12_PIPELINE_STATE_STREAM_DESC sd{b1.size(),b1.data()};
                        ID3D12PipelineState* t{};oneViewport=p.create(p.device,&sd,IID_PPV_ARGS(&t));if(t)t->Release();}
                    Log("[single-pass]   views on one viewport %08X\n",unsigned(oneViewport));
                    // One part of the engine's description changed at a time, original shaders, two views.
                    const auto variant=[&](const char* what,const std::function<void(D3D12_PIPELINE_STATE_SUBOBJECT_TYPE,uint8_t*)>& edit,
                                           bool dropPs)->HRESULT {
                        std::vector<uint8_t> edited;
                        std::vector<D3D12_INPUT_ELEMENT_DESC> inputs;
                        if(!cvr::stereo::EditPipelineStream(p.Desc(),edited,[&](D3D12_PIPELINE_STATE_SUBOBJECT_TYPE t,uint8_t* v){
                            if(t==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT && inputs.empty()) {   // own copy before edits
                                D3D12_INPUT_LAYOUT_DESC d{};std::memcpy(&d,v,sizeof(d));
                                if(d.pInputElementDescs)inputs.assign(d.pInputElementDescs,d.pInputElementDescs+d.NumElements);
                                d.pInputElementDescs=inputs.data();std::memcpy(v,&d,sizeof(d));
                            }
                            edit(t,v);
                        }))return E_ABORT;
                        const D3D12_PIPELINE_STATE_STREAM_DESC ed{edited.size(),edited.data()};
                        std::vector<uint8_t> b;
                        if(!BuildViewInstancedStream(ed,vs,layout,b,dropPs?D3D12_SHADER_BYTECODE{}:ps,twinRoot))return E_ABORT;
                        const D3D12_PIPELINE_STATE_STREAM_DESC sd{b.size(),b.data()};ID3D12PipelineState* t{};
                        const HRESULT r=p.create(p.device,&sd,IID_PPV_ARGS(&t));if(t)t->Release();
                        Log("[single-pass]   views, %s: %08X\n",what,unsigned(r));
                        return r;
                    };
                    using T=D3D12_PIPELINE_STATE_SUBOBJECT_TYPE;
                    variant("no pixel shader, no targets",[](T t,uint8_t* v){
                        if(t==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS){D3D12_SHADER_BYTECODE e{};std::memcpy(v,&e,sizeof(e));}
                        if(t==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS){D3D12_RT_FORMAT_ARRAY a{};std::memcpy(v,&a,sizeof(a));}},true);
                    variant("every input per vertex",[](T t,uint8_t* v){
                        if(t!=D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT)return;
                        D3D12_INPUT_LAYOUT_DESC d{};std::memcpy(&d,v,sizeof(d));
                        for(UINT i=0;i<d.NumElements;++i){auto& e=const_cast<D3D12_INPUT_ELEMENT_DESC&>(d.pInputElementDescs[i]);
                            e.InputSlotClass=D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA;e.InstanceDataStepRate=0;}},false);
                    variant("no stencil",[](T t,uint8_t* v){
                        if(t!=D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL)return;
                        D3D12_DEPTH_STENCIL_DESC d{};std::memcpy(&d,v,sizeof(d));d.StencilEnable=FALSE;std::memcpy(v,&d,sizeof(d));},false);
                    variant("no input layout",[](T t,uint8_t* v){
                        if(t!=D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_INPUT_LAYOUT)return;
                        D3D12_INPUT_LAYOUT_DESC d{};std::memcpy(v,&d,sizeof(d));},false);
                    variant("pixel shader, no targets",[](T t,uint8_t* v){
                        if(t==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS){D3D12_RT_FORMAT_ARRAY a{};std::memcpy(v,&a,sizeof(a));}},false);
                    variant("no pixel shader, targets kept",[](T t,uint8_t* v){
                        if(t==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS){D3D12_SHADER_BYTECODE e{};std::memcpy(v,&e,sizeof(e));}},true);
                    variant("targets RGBA16F",[](T t,uint8_t* v){
                        if(t!=D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS)return;
                        D3D12_RT_FORMAT_ARRAY a{};std::memcpy(&a,v,sizeof(a));
                        for(UINT i=0;i<a.NumRenderTargets;++i)a.RTFormats[i]=DXGI_FORMAT_R16G16B16A16_FLOAT;std::memcpy(v,&a,sizeof(a));},false);
                    variant("no depth stencil",[](T t,uint8_t* v){
                        if(t==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL){D3D12_DEPTH_STENCIL_DESC d{};std::memcpy(v,&d,sizeof(d));}
                        if(t==D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DEPTH_STENCIL_FORMAT){DXGI_FORMAT f=DXGI_FORMAT_UNKNOWN;std::memcpy(v,&f,sizeof(f));}},false);
                    variant("one target",[](T t,uint8_t* v){
                        if(t!=D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_RENDER_TARGET_FORMATS)return;
                        D3D12_RT_FORMAT_ARRAY a{};std::memcpy(&a,v,sizeof(a));a.NumRenderTargets=std::min<UINT>(a.NumRenderTargets,1);
                        std::memcpy(v,&a,sizeof(a));},false);
                    // the pixel shader's signatures, to compare with a vertex shader's outputs
                    static std::atomic<int> dumped{0};
                    if(dumped.fetch_add(1)<2) {
                        const uint64_t hv=Fnv(vs.pShaderBytecode,vs.BytecodeLength),hp=ps.BytecodeLength?Fnv(ps.pShaderBytecode,ps.BytecodeLength):0;
                        Log("[single-pass]   shaders: vs %016llx ps %016llx\n",static_cast<unsigned long long>(hv),static_cast<unsigned long long>(hp));
                        const auto save=[&](uint64_t h,const D3D12_SHADER_BYTECODE& c){
                            wchar_t n[80];swprintf_s(n,L"refused_%016llx.dxil",static_cast<unsigned long long>(h));
                            std::ofstream(CacheDir()+n,std::ios::binary).write(static_cast<const char*>(c.pShaderBytecode),std::streamsize(c.BytecodeLength));};
                        save(hv,vs);if(hp)save(hp,ps);
                    }
                }
            }
        }else Error("pipeline stream could not take the views");
    }
    Count(result==2?3:4)++;
    Count(10)+=uint64_t(std::chrono::duration<double,std::milli>(std::chrono::steady_clock::now()-t0).count()+.5);
    p.state[variant].store(result,std::memory_order_release);
}

void Worker() {
    SetThreadPriority(GetCurrentThread(),THREAD_PRIORITY_BELOW_NORMAL);
    SetThreadDescription(GetCurrentThread(),L"cvr single-pass twins");
    ViewTwinCompiler compiler;std::string error;
    const bool ready=compiler.Load(cvr::paths::Dir(cvr::paths::Folder::Root)+L"dxc\\",error);
    compilerState.store(ready?1:2,std::memory_order_release);
    if(!ready){Error(("compiler: "+error).c_str());}
    else Log("[single-pass] DXC loaded; twins cached in %ls\n",CacheDir().c_str());
    for(;;) {
        Job job;
        {
            std::unique_lock lock(jobMutex);
            jobSignal.wait(lock,[]{return !jobs.empty();});
            job=std::move(jobs.front());jobs.pop_front();
        }
        if(!ready){job.pipeline->state[job.variant].store(3,std::memory_order_release);Count(4)++;continue;}
        BuildPipeline(compiler,*job.pipeline.Get(),job.variant);
    }
}
void Queue(Pipeline* p,int variant) {
    std::call_once(workerOnce,[]{for(int i=0;i<3;++i)std::thread(Worker).detach();});   // three builders, below normal priority
    Count(2)++;
    {std::lock_guard lock(jobMutex);jobs.push_back({p,variant});}
    jobSignal.notify_one();
}

bool Wanted() {return CyberpunkVR_SinglePassPipelines.load(std::memory_order_relaxed)!=0;}

// The bytecode the owned descriptions point at, one copy per shader (a DXIL/DXBC container is keyed by its own
// 16-byte hash and its size; ~4,800 pipelines share a few hundred shaders).
std::mutex blobMutex;
std::unordered_map<std::string,std::weak_ptr<const std::vector<uint8_t>>> blobs;
std::shared_ptr<const std::vector<uint8_t>> Shared(const D3D12_SHADER_BYTECODE& code) {
    if(!code.pShaderBytecode || !code.BytecodeLength)return nullptr;
    const auto* b=static_cast<const uint8_t*>(code.pShaderBytecode);
    std::string key=code.BytecodeLength>=20 && !std::memcmp(b,"DXBC",4)?std::string(reinterpret_cast<const char*>(b+4),16):
                    std::to_string(Fnv(b,code.BytecodeLength));
    key+=std::to_string(code.BytecodeLength);
    std::lock_guard lock(blobMutex);
    auto& slot=blobs[key];
    if(auto shared=slot.lock())return shared;
    auto made=std::make_shared<const std::vector<uint8_t>>(b,b+code.BytecodeLength);
    slot=made;Count(9)+=code.BytecodeLength;
    return made;
}
void Register(ID3D12Device* device,ID3D12PipelineState* pso,CreateStreamFn create,ComPtr<Pipeline>& p) {
    if(FAILED(pso->SetPrivateDataInterface(pipelineKey,p.Get())))return;
    Count(1)++;
    Count(9)+=p->stream.stream.size()+p->converted.size();
    (void)device;(void)create;
}
}

void RootCreated(ID3D12Device* device,ID3D12RootSignature* root,const void* blob,size_t bytes) {
    if(!device || !root || !blob || !bytes || !Wanted())return;
    ComPtr<ID3D12VersionedRootSignatureDeserializer> des;
    if(FAILED(D3D12CreateVersionedRootSignatureDeserializer(blob,bytes,IID_PPV_ARGS(&des))))return;
    const D3D12_VERSIONED_ROOT_SIGNATURE_DESC* v{};
    if(FAILED(des->GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1,&v)) || !v)return;
    const auto& d=v->Desc_1_1;
    if(d.NumParameters!=36 || !TableIsCameraBlock(d.pParameters[0],D3D12_SHADER_VISIBILITY_VERTEX) ||
       !TableIsCameraBlock(d.pParameters[18],D3D12_SHADER_VISIBILITY_PIXEL))return;
    for(UINT i=0;i<d.NumParameters;++i)if(d.pParameters[i].ParameterType!=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE)return;
    // The twin: the same 36 tables, then b0..b3 in space 10 for the vertex (36) and the pixel stage (37).
    std::vector<D3D12_ROOT_PARAMETER1> params(d.pParameters,d.pParameters+d.NumParameters);
    D3D12_DESCRIPTOR_RANGE1 ranges[2]{d.pParameters[0].DescriptorTable.pDescriptorRanges[0],d.pParameters[18].DescriptorTable.pDescriptorRanges[0]};
    for(auto& r:ranges)r.RegisterSpace=kTwinSpace;
    for(int k=0;k<2;++k) {
        D3D12_ROOT_PARAMETER1 p{};p.ParameterType=D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
        p.DescriptorTable={1,&ranges[k]};p.ShaderVisibility=k?D3D12_SHADER_VISIBILITY_PIXEL:D3D12_SHADER_VISIBILITY_VERTEX;
        params.push_back(p);
    }
    D3D12_VERSIONED_ROOT_SIGNATURE_DESC twinDesc{D3D_ROOT_SIGNATURE_VERSION_1_1};
    twinDesc.Desc_1_1={UINT(params.size()),params.data(),d.NumStaticSamplers,d.pStaticSamplers,d.Flags};
    ComPtr<ID3DBlob> serialized,error;
    if(FAILED(D3D12SerializeVersionedRootSignature(&twinDesc,&serialized,&error))){Error("twin root: serialization failed");return;}
    ComPtr<ID3D12RootSignature> twin;
    if(FAILED(device->CreateRootSignature(0,serialized->GetBufferPointer(),serialized->GetBufferSize(),IID_PPV_ARGS(&twin)))) {
        Error("twin root: creation failed");return;
    }
    std::lock_guard lock(rootMutex);
    roots[root].twin=twin;
    Count(0)=roots.size();
    rootCount.store(uint32_t(roots.size()),std::memory_order_release);
    Activate();
    Log("[single-pass] material root %p: twin %p (38 tables)\n",static_cast<void*>(root),static_cast<void*>(twin.Get()));
}

ID3D12RootSignature* TwinRoot(ID3D12RootSignature* root) {
    if(!root || !rootCount.load(std::memory_order_acquire))return nullptr;
    std::lock_guard lock(rootMutex);
    const auto it=roots.find(root);
    return it==roots.end()?nullptr:it->second.twin.Get();
}
bool Available() {return compilerState.load(std::memory_order_acquire)!=2 && rootCount.load(std::memory_order_acquire)!=0;}

void GraphicsCreated(ID3D12Device* device,const D3D12_GRAPHICS_PIPELINE_STATE_DESC& desc,ID3D12PipelineState* pso,CreateStreamFn create) {
    if(!pso || !create || !TwinRoot(desc.pRootSignature) || !desc.VS.BytecodeLength || desc.HS.BytecodeLength ||
       desc.DS.BytecodeLength || desc.GS.BytecodeLength || desc.StreamOutput.NumEntries)return;
    ComPtr<Pipeline> p;p.Attach(new Pipeline);
    p->device=device;p->create=create;p->root=desc.pRootSignature;
    if(!OwnGraphicsDesc(desc,p->graphics))return;
    p->vsBlob=Shared(desc.VS);p->psBlob=Shared(desc.PS);
    p->graphics.desc.VS={p->vsBlob->data(),p->vsBlob->size()};
    p->graphics.desc.PS=p->psBlob?D3D12_SHADER_BYTECODE{p->psBlob->data(),p->psBlob->size()}:D3D12_SHADER_BYTECODE{};
    p->graphics.parts.shaders.clear();p->graphics.parts.shaders.shrink_to_fit();
    if(!GraphicsDescStream(p->graphics.desc,p->converted))return;
    p->vs=p->graphics.desc.VS;p->ps=p->graphics.desc.PS;
    for(UINT i=0;i<desc.InputLayout.NumElements && desc.InputLayout.pInputElementDescs;++i)
        p->slots|=1u<<(desc.InputLayout.pInputElementDescs[i].InputSlot&31);
    Register(device,pso,create,p);
}

void StreamCreated(ID3D12Device* device,const D3D12_PIPELINE_STATE_STREAM_DESC& desc,ID3D12PipelineState* pso,CreateStreamFn create) {
    if(!pso || !create || !rootCount.load(std::memory_order_acquire))return;
    PipelineStreamInfo info;
    if(!ReadPipelineStream(desc,info) || !TwinRoot(info.root) || !info.vs.BytecodeLength || info.otherStages ||
       info.viewInstancing || info.streamOutput)return;
    ComPtr<Pipeline> p;p.Attach(new Pipeline);
    p->device=device;p->create=create;p->root=info.root;
    if(!OwnPipelineStream(desc,p->stream))return;
    auto* base=p->stream.stream.data();
    if(p->stream.offsets.vs==SIZE_MAX)return;
    std::memcpy(&p->vs,base+p->stream.offsets.vs,sizeof(p->vs));
    p->vsBlob=Shared(p->vs);p->vs={p->vsBlob->data(),p->vsBlob->size()};std::memcpy(base+p->stream.offsets.vs,&p->vs,sizeof(p->vs));
    if(p->stream.offsets.ps!=SIZE_MAX) {
        D3D12_SHADER_BYTECODE ps{};std::memcpy(&ps,base+p->stream.offsets.ps,sizeof(ps));
        if(ps.BytecodeLength){p->psBlob=Shared(ps);ps={p->psBlob->data(),p->psBlob->size()};std::memcpy(base+p->stream.offsets.ps,&ps,sizeof(ps));}
    }
    p->stream.parts.shaders.clear();p->stream.parts.shaders.shrink_to_fit();
    for(UINT i=0;i<info.input.NumElements && info.input.pInputElementDescs;++i)
        p->slots|=1u<<(info.input.pInputElementDescs[i].InputSlot&31);
    if(p->stream.offsets.ps!=SIZE_MAX)std::memcpy(&p->ps,base+p->stream.offsets.ps,sizeof(p->ps));
    Register(device,pso,create,p);
}

Pipeline* Find(ID3D12PipelineState* pso) {
    if(!pso || !rootCount.load(std::memory_order_relaxed))return nullptr;
    Pipeline* p{};UINT size=sizeof(p);
    if(FAILED(pso->GetPrivateData(pipelineKey,&size,&p)) || size!=sizeof(p) || !p)return nullptr;
    p->Release();   // GetPrivateData added a reference; the PSO keeps the record alive
    return p;
}
ID3D12RootSignature* RootOf(const Pipeline* p) {return p?p->root:nullptr;}
uint32_t InputSlots(const Pipeline* p) {return p?p->slots:~0u;}

ID3D12PipelineState* Twin(Pipeline* p,ID3D12PipelineState* engine,ID3D12PipelineState* bound) {
    if(!p)return nullptr;
    int variant=0;
    if(bound!=engine) {
        // Bound with a replacement: only a hidden-area twin is known here (a sight twin is not).
        int replaced=p->replaced.load(std::memory_order_relaxed);
        if(replaced<0) {
            replaced=cvr::hidden_area::shaders::BoundCode(p->vs).pShaderBytecode!=p->vs.pShaderBytecode ||
                     (p->ps.BytecodeLength && cvr::hidden_area::shaders::BoundCode(p->ps).pShaderBytecode!=p->ps.pShaderBytecode);
            p->replaced.store(replaced,std::memory_order_relaxed);
        }
        if(!replaced){Count(11)++;return nullptr;}
        variant=1;
    }
    const int s=p->state[variant].load(std::memory_order_acquire);
    if(s==2)return p->twin[variant].Get();
    if(s==0 && compilerState.load(std::memory_order_relaxed)!=2) {
        int idle=0;
        if(p->state[variant].compare_exchange_strong(idle,1,std::memory_order_acq_rel))Queue(p,variant);
    }
    return nullptr;
}
}
