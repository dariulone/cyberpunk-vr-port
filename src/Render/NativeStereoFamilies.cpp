#include "Render/NativeStereoFamilies.hpp"
#include "Render/SkinningReadPlan.hpp"
#include "Render/AlphaDepthContract.hpp"
#include "Render/InstanceRebase.hpp"
#include "Render/ViewInstancedPipeline.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/NativeDepthPolicy.hpp"
#include "Render/ObservationProfile.hpp"
#include <bcrypt.h>
#include <json/json.h>
#include <algorithm>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <map>
#include <mutex>
#include <stdexcept>
#include <tuple>
#if CVR_RUST_VRIK
#include "Rust/JsonTree.hpp"
#endif
using Microsoft::WRL::ComPtr;
#if CVR_RUST_VRIK
extern "C" {
// The exported state is Rust's (rust/crates/cvr-render/src/native_state_exports.rs), declared in the header; the records
// are written by rust/crates/cvr-render/src/families_native.rs.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's rows are rust/crates/cvr-render/src/render_moved_exports.rs'
__declspec(dllexport) cvr::stereo::families::Record CyberpunkVR_StereoFamilyRecords[cvr::stereo::families::Capacity]{};
#endif
}
// THE REGISTRY IS RUST'S (rust/crates/cvr-render/src/families_native.rs): the manifest's assets, the roots' identity, the
// pipelines' observation and registration, the lists' bound families and their draws. This half reads the manifest with
// jsoncpp and keeps a mirror of each family and paired root for its C++ consumers, made through the host below.
namespace cvr::stereo::families {
namespace {
struct RustFamilyBytes {const uint8_t* p;size_t n;};
struct RustFamilyAsset {
    uint32_t index,vsBytes,psBytes,cameraStages;RustFamilyBytes vsHash,psHash,vs,ps,cameraVs,cameraPs;
    const PairedBinding* pairs;size_t pairCount;const ResourceUse* uses;size_t useCount;
};
struct RustFamilyManifest {const RustFamilyAsset* assets;size_t assetCount;char manifestHash[65];};
struct RustVariant {ID3D12RootSignature* root;const uint8_t* serialized;size_t serializedLength;const PairedBindingLocation* bindings;
    size_t bindingCount;UINT scalars[6];};
struct RustFamily {
    uint64_t id;UINT asset,record;RustFamilyBytes rootSha,vertexSha,pixelSha;ID3D12RootSignature* sourceRoot;ID3D12PipelineState* pipeline;
    ID3D12PipelineState* nativeCameraPipeline;UINT nativeCameraStages,skinningReadContract,alphaDepthContract;
    bool instanceRebaseSafe,instanceOrdinalIndependent;const ResourceUse* uses;size_t useCount;PassContract pass;
};
static_assert(sizeof(RustFamilyAsset)==144 && sizeof(RustFamilyManifest)==88,"mirrored by stereo_family_manifest.rs");
static_assert(sizeof(RustVariant)==64 && sizeof(RustFamily)==232 && sizeof(PassContract)==112,"mirrored by families_native.rs");
struct Host {
    void*(*load)(const wchar_t*,size_t,const RustFamilyManifest**,char*,size_t);
    void*(*variant)(const RustVariant*);
    void*(*family)(const RustFamily*,const void*);
    void(*free)(uint32_t,void*);
};
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin makes no C++ mirrors (families_native.rs keeps the families)
using SharedVariant=std::shared_ptr<const RootVariant>;
using SharedFamily=std::shared_ptr<const Family>;
std::string Text(RustFamilyBytes b){return std::string(reinterpret_cast<const char*>(b.p),b.n);}
#endif
void Require(bool yes,const char* reason){if(!yes)throw std::runtime_error(reason);}
}
}
extern "C" {
void* cvr_nsf_load(const cvr::json_tree::Node*,size_t,const wchar_t*,size_t,const uint8_t*,size_t,
    const cvr::stereo::families::RustFamilyManifest**,char*,size_t);
void cvr_nsf2_attach(const void*);
bool cvr_nsf2_enabled();
void cvr_nsf2_root_created(ID3D12RootSignature*,UINT,const void*,size_t);
void cvr_nsf2_graphics_created(ID3D12Device*,const D3D12_GRAPHICS_PIPELINE_STATE_DESC*,ID3D12PipelineState*);
void cvr_nsf2_stream_created(ID3D12Device*,const D3D12_PIPELINE_STATE_STREAM_DESC*,ID3D12PipelineState*);
void cvr_nsf2_pipeline_bound(ID3D12GraphicsCommandList*,ID3D12PipelineState*);
void cvr_nsf2_root_bound(ID3D12GraphicsCommandList*,ID3D12RootSignature*);
void cvr_nsf2_reset(ID3D12GraphicsCommandList*,ID3D12PipelineState*);
void cvr_nsf2_observe_draw(ID3D12GraphicsCommandList*,UINT,int);
const void* cvr_nsf2_lookup(void*,uint32_t,void**);
}
namespace cvr::stereo::families {
namespace {
const Host kHost{
    // Load: the manifest's text read and parsed by jsoncpp, its tree handed to stereo_family_manifest.rs.
    [](const wchar_t* path,size_t length,const RustFamilyManifest** view,char* failure,size_t capacity)->void*{
        try{
            const std::filesystem::path file(std::wstring(path,length));
            const auto size=std::filesystem::file_size(file);Require(size>0 && size<=8*1024*1024,"Invalid family manifest size");
            Json::CharReaderBuilder builder;builder["collectComments"]=false;builder["failIfExtra"]=true;builder["rejectDupKeys"]=true;
            Json::Value manifest;std::string errors,text(size,'\0');std::ifstream stream(file,std::ios::binary);stream.read(text.data(),text.size());
            Require(bool(stream),"Cannot read family manifest");
            std::unique_ptr<Json::CharReader> parser(builder.newCharReader());
            Require(parser->parse(text.data(),text.data()+text.size(),&manifest,&errors),"Invalid family manifest JSON");
            std::vector<json_tree::Node> nodes(1);json_tree::Flatten(manifest,nodes,0);
            const auto dir=file.parent_path().wstring();
            return cvr_nsf_load(nodes.data(),nodes.size(),dir.data(),dir.size(),reinterpret_cast<const uint8_t*>(text.data()),text.size(),
                view,failure,capacity);
        }catch(const std::exception& e){strncpy_s(failure,capacity,e.what(),_TRUNCATE);return nullptr;}},
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin makes no C++ mirrors (families_native.rs keeps the families)
    [](const RustVariant* v)->void*{
        try{auto variant=std::make_shared<RootVariant>();variant->root=v->root;auto& plan=variant->plan;
            plan.serialized.assign(v->serialized,v->serialized+v->serializedLength);plan.bindings.assign(v->bindings,v->bindings+v->bindingCount);
            plan.parameters={v->scalars[0],v->scalars[1]};plan.descriptors={v->scalars[2],v->scalars[3]};
            plan.originalParameters=v->scalars[4];plan.rootDwords=v->scalars[5];
            return new SharedVariant(std::move(variant));}catch(...){return nullptr;}},
    [](const RustFamily* v,const void* variant)->void*{
        try{auto f=std::make_shared<Family>();f->id=v->id;f->asset=v->asset;f->record=v->record;
            f->rootSha256=Text(v->rootSha);f->vertexSha256=Text(v->vertexSha);f->pixelSha256=Text(v->pixelSha);
            f->sourceRoot=v->sourceRoot;f->pairedRoot=*static_cast<const SharedVariant*>(variant);f->pipeline=v->pipeline;
            f->nativeCameraPipeline=v->nativeCameraPipeline;f->nativeCameraStages=v->nativeCameraStages;
            f->skinningReadContract=v->skinningReadContract;f->alphaDepthContract=v->alphaDepthContract;
            f->uses.assign(v->uses,v->uses+v->useCount);f->instanceRebaseSafe=v->instanceRebaseSafe;
            f->instanceOrdinalIndependent=v->instanceOrdinalIndependent;f->pass=v->pass;
            return new SharedFamily(std::move(f));}catch(...){return nullptr;}},
    [](uint32_t kind,void* mirror){if(kind)delete static_cast<SharedFamily*>(mirror);else delete static_cast<SharedVariant*>(mirror);},
#else
    nullptr,nullptr,nullptr,
#endif
};
[[maybe_unused]] const bool kAttached=(cvr_nsf2_attach(&kHost),true);
// Lookup and Bound: the family's mirror copied while its owner is held.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
SharedFamily Mirror(void* object,uint32_t which){
    void* held{};const auto* mirror=static_cast<const SharedFamily*>(cvr_nsf2_lookup(object,which,&held));
    SharedFamily family=mirror?*mirror:nullptr;if(held)static_cast<IUnknown*>(held)->Release();return family;
}
#endif
}
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
bool Enabled(){return cvr_nsf2_enabled();}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's hooks and detours call families_native.rs themselves
void RootCreated(ID3D12RootSignature* root,UINT node,const void* bytes,size_t size){cvr_nsf2_root_created(root,node,bytes,size);}
void GraphicsCreated(ID3D12Device* device,const D3D12_GRAPHICS_PIPELINE_STATE_DESC& source,ID3D12PipelineState* original){
    cvr_nsf2_graphics_created(device,&source,original);
}
void StreamCreated(ID3D12Device* device,const D3D12_PIPELINE_STATE_STREAM_DESC& source,ID3D12PipelineState* original){
    cvr_nsf2_stream_created(device,&source,original);
}
bool NativeDepthCompatible(ID3D12PipelineState* pso){return NativeDepthStatus(pso)==1;}
uint32_t NativeDepthStatus(ID3D12PipelineState* pso){
    constexpr GUID key{0x0db3e624,0x37df,0x4491,{0xa6,0x91,0xa1,0x1e,0x6d,0x2d,0x5e,0x50}};
    uint32_t status{};UINT bytes=sizeof(status);return pso && SUCCEEDED(pso->GetPrivateData(key,&bytes,&status)) && bytes==sizeof(status)?status:0;
}
uint32_t NativeGeometryStatus(ID3D12PipelineState* pso){
    constexpr GUID key{0x0db3e624,0x37df,0x4491,{0xa6,0x91,0xa1,0x1e,0x6d,0x2d,0x5e,0x51}};
    uint32_t status{};UINT bytes=sizeof(status);return pso && SUCCEEDED(pso->GetPrivateData(key,&bytes,&status)) && bytes==sizeof(status)?status:0;
}
void PipelineBound(ID3D12GraphicsCommandList* list,ID3D12PipelineState* pso){cvr_nsf2_pipeline_bound(list,pso);}
void Reset(ID3D12GraphicsCommandList* list,ID3D12PipelineState* pso){cvr_nsf2_reset(list,pso);}
void ObserveDraw(ID3D12GraphicsCommandList* list,UINT node,int side){cvr_nsf2_observe_draw(list,node,side);}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
void RootBound(ID3D12GraphicsCommandList* list,ID3D12RootSignature* root){cvr_nsf2_root_bound(list,root);}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
std::shared_ptr<const Family> Lookup(ID3D12PipelineState* pso){return Mirror(pso,0);}
#endif
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not call it; the Rust test variants do
std::shared_ptr<const Family> Bound(ID3D12GraphicsCommandList* list){return Mirror(list,1);}
#endif
}
#else
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoFamilyAssetStatus{},CyberpunkVR_StereoFamilyAssetCount{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoFamilyNativeCameras{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoFamilySeq{},CyberpunkVR_StereoFamilyCount{},CyberpunkVR_StereoFamilyOverflow{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoFamilyMisses{},CyberpunkVR_StereoFamilyFailures{},CyberpunkVR_StereoFamilyHits{},CyberpunkVR_StereoFamilyRootHits{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoFamilyObserve{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_StereoFamilyBindingFailures{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_StereoFamilyUnmatched[2]{},CyberpunkVR_StereoFamilyRootMismatch[2]{};
__declspec(dllexport) uint32_t CyberpunkVR_StereoFamilyRecordBytes=sizeof(cvr::stereo::families::Record);
__declspec(dllexport) uint32_t CyberpunkVR_StereoFamilyNativeCameraRecordBytes=16;
__declspec(dllexport) uint64_t CyberpunkVR_StereoFamilyNativeCameraPipelines[cvr::stereo::families::Capacity][2]{};
__declspec(dllexport) uint32_t CyberpunkVR_StereoFamilySkinningReadContracts[cvr::stereo::families::Capacity]{};
__declspec(dllexport) uint32_t CyberpunkVR_StereoFamilyAlphaDepthContracts[cvr::stereo::families::Capacity]{};
__declspec(dllexport) cvr::stereo::families::Record CyberpunkVR_StereoFamilyRecords[cvr::stereo::families::Capacity]{};
__declspec(dllexport) char CyberpunkVR_StereoFamilyError[256]{};
__declspec(dllexport) char CyberpunkVR_StereoFamilyManifestHash[65]{};
}
namespace cvr::stereo::families {
namespace {
constexpr GUID rootKey{0x83b20c11,0x913f,0x4406,{0xa4,0xa2,0xfa,0x7d,0x8b,0x98,0x54,0x60}};
constexpr GUID familyKey{0x721b72f4,0xda5d,0x4562,{0x96,0x3a,0x23,0xd9,0x9b,0x25,0xb6,0x13}};
constexpr GUID boundFamilyKey{0x753cd6f2,0xa2af,0x44f8,{0x96,0x0b,0x73,0x06,0x88,0x0b,0x79,0x4c}};
constexpr GUID boundRootKey{0x47b7d283,0x9884,0x46f8,{0x99,0x4d,0xe2,0x4e,0x3e,0x8e,0xa0,0x43}};
constexpr GUID nativeDepthKey{0x0db3e624,0x37df,0x4491,{0xa6,0x91,0xa1,0x1e,0x6d,0x2d,0x5e,0x50}};
constexpr GUID nativeGeometryKey{0x0db3e624,0x37df,0x4491,{0xa6,0x91,0xa1,0x1e,0x6d,0x2d,0x5e,0x51}};
template<class T> class Owner final:public IUnknown {
    std::atomic<ULONG> refs{1};
public:
    T data;
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID id,void** out)override{if(!out)return E_POINTER;*out=nullptr;if(id!=__uuidof(IUnknown))return E_NOINTERFACE;*out=this;AddRef();return S_OK;}
    ULONG STDMETHODCALLTYPE AddRef()override{return ++refs;}
    ULONG STDMETHODCALLTYPE Release()override{auto n=--refs;if(!n)delete this;return n;}
};
template<class T> ComPtr<T> Private(ID3D12Object* object,REFGUID key) {
    ComPtr<T> result;if(!object)return result;IUnknown* raw{};UINT size=sizeof(raw);
    if(SUCCEEDED(object->GetPrivateData(key,&size,&raw)) && size==sizeof(raw) && raw)result.Attach(static_cast<T*>(raw));return result;
}
struct RootData {
    UINT node{};std::vector<uint8_t> bytes;std::string digest;std::mutex mutex;
    // D3D12 destroys private data while holding its root-object lock. Owning
    // other roots here can deadlock on their Release (or cycle on an identical
    // cached root). Families own variants; the original root keeps weak keys.
    std::map<std::vector<UINT>,std::weak_ptr<const RootVariant>> variants;
};
using RootOwner=Owner<RootData>;
using FamilyOwner=Owner<std::shared_ptr<const Family>>;
struct Asset {UINT index{},vsBytes{},psBytes{},cameraStages{};std::string vsHash,psHash;std::vector<uint8_t> vs,ps,cameraVs,cameraPs;std::vector<PairedBinding> pairs;std::vector<ResourceUse> uses;};
std::map<std::pair<std::string,std::string>,Asset> assets;
std::mutex recordsMutex;
std::atomic<uint64_t> nextId{1};
void Require(bool yes,const char* reason){if(!yes)throw std::runtime_error(reason);}
std::string Hash(const void* data,size_t size) {
    Require(data && size && size<=8*1024*1024,"Invalid shader/root/manifest size");
    struct Provider {BCRYPT_ALG_HANDLE handle{};Provider(){if(BCryptOpenAlgorithmProvider(&handle,BCRYPT_SHA256_ALGORITHM,nullptr,0)<0)handle=nullptr;}~Provider(){if(handle)BCryptCloseAlgorithmProvider(handle,0);}};
    static Provider provider;Require(provider.handle!=nullptr,"SHA256 provider unavailable");uint8_t bytes[32]{};
    Require(BCryptHash(provider.handle,nullptr,0,static_cast<PUCHAR>(const_cast<void*>(data)),ULONG(size),bytes,sizeof(bytes))>=0,"SHA256 failed");
    std::string result;for(auto b:bytes){result+="0123456789abcdef"[b>>4];result+="0123456789abcdef"[b&15];}return result;
}
bool Digest(const std::string& s){return s.size()==64 && std::all_of(s.begin(),s.end(),[](char c){return (c>='0' && c<='9') || (c>='a' && c<='f');});}
UINT Number(const Json::Value& value,const char* name,UINT limit=UINT_MAX) {
    Require(value.isMember(name) && value[name].isUInt() && value[name].asUInt()<=limit,"Invalid manifest number");return value[name].asUInt();
}
std::vector<uint8_t> Binary(const std::filesystem::path& dir,const Json::Value& value) {
    const auto digest=value["sha256"].asString();const auto name=value["file"].asString();const auto size=Number(value,"bytes",1024*1024);
    Require(size>=32 && Digest(digest) && name==digest+".dxil","Invalid shader filename/hash");
    const auto path=dir/name;Require(std::filesystem::file_size(path)==size,"Shader file size differs");
    std::ifstream file(path,std::ios::binary);std::vector<uint8_t> bytes(size);file.read(reinterpret_cast<char*>(bytes.data()),size);
    Require(bool(file) && Hash(bytes.data(),bytes.size())==digest,"Shader file hash differs");return bytes;
}
void Load(const std::filesystem::path& path) {
    const auto size=std::filesystem::file_size(path);Require(size>0 && size<=8*1024*1024,"Invalid family manifest size");
    Json::CharReaderBuilder builder;builder["collectComments"]=false;builder["failIfExtra"]=true;builder["rejectDupKeys"]=true;
    Json::Value manifest;std::string errors,text(size,'\0');std::ifstream file(path,std::ios::binary);file.read(text.data(),text.size());Require(bool(file),"Cannot read family manifest");
    std::unique_ptr<Json::CharReader> parser(builder.newCharReader());
    Require(parser->parse(text.data(),text.data()+text.size(),&manifest,&errors),"Invalid family manifest JSON");
    Require(Number(manifest,"schema")==1 && manifest["purpose"]=="development-linkage-only" && manifest["families"].isArray() &&
        manifest["families"].size()>0 && manifest["families"].size()<=512,"Unsupported family manifest");
    decltype(assets) pending;UINT64 bytes{};
    for(const auto& row:manifest["families"]) {
        Asset a;a.index=UINT(pending.size()+1);a.vsHash=row["nativeVs"].asString();a.psHash=row["nativePs"].asString();
        a.vsBytes=Number(row,"nativeVsBytes",1024*1024);a.psBytes=Number(row,"nativePsBytes",1024*1024);
        Require(Digest(a.vsHash) && a.vsBytes>=32 && ((a.psHash.empty() && !a.psBytes) || (Digest(a.psHash) && a.psBytes>=32)),"Invalid original shader identity");
        a.vs=Binary(path.parent_path(),row["vs"]);if(a.psBytes)a.ps=Binary(path.parent_path(),row["ps"]);
        bytes+=a.vs.size()+a.ps.size();Require(bytes<=128*1024*1024,"Shader package exceeds memory bound");
        Require(row["pairs"].isArray() && row["pairs"].size()<=512 && row["uses"].isArray() && row["uses"].size()<=1024,"Invalid resource contract");
        for(const auto& p:row["pairs"]) {
            PairedBinding b{D3D12_DESCRIPTOR_RANGE_TYPE(Number(p,"type",3)),D3D12_SHADER_VISIBILITY(Number(p,"visibility",5)),Number(p,"register",0x7ffffffe),Number(p,"space",65535)};
            Require((b.type==D3D12_DESCRIPTOR_RANGE_TYPE_SRV || b.type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV) &&
                (b.visibility==D3D12_SHADER_VISIBILITY_VERTEX || (a.psBytes && b.visibility==D3D12_SHADER_VISIBILITY_PIXEL)) && b.privateSpace,"Invalid pair contract");
            a.pairs.push_back(b);
        }
        for(const auto& u:row["uses"]) {
            ResourceUse use{Number(u,"type",3),Number(u,"visibility",5),Number(u,"space"),Number(u,"register"),Number(u,"count"),Number(u,"bytes",65536)};
            Require(use.count && (use.visibility==1 || (a.psBytes && use.visibility==5)),"Invalid resource use");a.uses.push_back(use);
        }
        for(const auto& b:a.pairs)Require(std::count_if(a.uses.begin(),a.uses.end(),[&](const auto& u){return u.type==UINT(b.type) && u.visibility==UINT(b.visibility) &&
            !u.space && u.shaderRegister==b.sourceRegister && u.count==1;})==1,"Pair has no unique original resource");
        if(row.isMember("nativeCameraVs")){
            const auto& camera=row["nativeCameraVs"];
            Require(a.vsHash=="17f7e646a02eeaa6ea496dd75a21a494588b29dd926a545fb52bee45dec68117" && !a.psBytes &&
                a.uses.size()==2 && camera["mainFirst"]==true && Number(camera,"eyeStride")==1024,"Unsupported native camera family");
            Require(!row.isMember("nativeCamera"),"Ambiguous native camera contract");
            a.cameraVs=Binary(path.parent_path(),camera);a.cameraStages=1;
        }else if(row.isMember("nativeCamera")){
            const auto& camera=row["nativeCamera"];
            Require(Number(camera,"schema")==1 && camera["mainFirst"]==true && Number(camera,"eyeStride")==1024 &&
                Number(camera,"bindingSize")==2048,"Unsupported native camera ABI");
            a.cameraStages=Number(camera,"stages",3);Require(a.cameraStages!=0,"Native camera stages missing");
            UINT expected{};
            for(const auto& use:a.uses)if(use.type==D3D12_DESCRIPTOR_RANGE_TYPE_CBV && !use.space && use.shaderRegister==1){
                const auto bit=use.visibility==D3D12_SHADER_VISIBILITY_VERTEX?1u:2u;
                Require(use.count==1 && use.bytes==848 && !(expected&bit),"Unsupported native stage camera binding");expected|=bit;
            }
            Require(expected==a.cameraStages && camera["vs"]["cameraPatched"].isBool() &&
                camera["vs"]["cameraPatched"].asBool()==bool(expected&1),"Native vertex camera contract differs");
            a.cameraVs=Binary(path.parent_path(),camera["vs"]);
            if(a.psBytes){
                Require(camera["ps"]["cameraPatched"].isBool() && camera["ps"]["cameraPatched"].asBool()==bool(expected&2),"Native pixel camera contract differs");
                a.cameraPs=Binary(path.parent_path(),camera["ps"]);
            }else Require(!camera.isMember("ps") && !(expected&2),"Unexpected native camera pixel shader");
        }
        bytes+=a.cameraVs.size()+a.cameraPs.size();Require(bytes<=128*1024*1024,"Camera shader package exceeds memory bound");
        Require(pending.emplace(std::make_pair(a.vsHash,a.psHash),std::move(a)).second,"Duplicate family");
    }
    assets=std::move(pending);CyberpunkVR_StereoFamilyAssetCount=UINT(assets.size());
    CyberpunkVR_StereoFamilyNativeCameras=UINT(std::count_if(assets.begin(),assets.end(),[](const auto& a){return !a.second.cameraVs.empty();}));
    const auto digest=Hash(text.data(),text.size());std::memcpy(CyberpunkVR_StereoFamilyManifestHash,digest.c_str(),65);
}
void Publish(Record row,Family* family=nullptr) {
    std::lock_guard lock(recordsMutex);const auto index=CyberpunkVR_StereoFamilyCount.load();
    if(index>=Capacity){CyberpunkVR_StereoFamilyOverflow=1;return;}
    CyberpunkVR_StereoFamilySeq.fetch_add(1);CyberpunkVR_StereoFamilyRecords[index]=row;
    if(family){family->record=index;
        CyberpunkVR_StereoFamilySkinningReadContracts[index]=family->skinningReadContract;
        CyberpunkVR_StereoFamilyAlphaDepthContracts[index]=family->alphaDepthContract;
        CyberpunkVR_StereoFamilyNativeCameraPipelines[index][0]=reinterpret_cast<uintptr_t>(family->nativeCameraPipeline.Get());
        CyberpunkVR_StereoFamilyNativeCameraPipelines[index][1]=family->nativeCameraStages;}
    CyberpunkVR_StereoFamilyCount.store(index+1);CyberpunkVR_StereoFamilySeq.fetch_add(1);
}
std::shared_ptr<const RootVariant> PairedRoot(ID3D12Device* device,ID3D12RootSignature* root,const Asset& asset) {
    auto owner=Private<RootOwner>(root,rootKey);if(!owner)return {};
    auto ordered=asset.pairs;std::sort(ordered.begin(),ordered.end(),[](const auto& a,const auto& b){
        return std::tie(a.visibility,a.type,a.sourceRegister,a.privateSpace)<std::tie(b.visibility,b.type,b.sourceRegister,b.privateSpace);});
    std::vector<UINT> key;for(const auto& b:ordered){key.push_back(b.visibility);key.push_back(b.type);key.push_back(b.sourceRegister);key.push_back(b.privateSpace);}
    std::lock_guard lock(owner->data.mutex);const auto old=owner->data.variants.find(key);
    if(old!=owner->data.variants.end()) {
        if(auto cached=old->second.lock()){++CyberpunkVR_StereoFamilyRootHits;return cached;}
        owner->data.variants.erase(old);
    }
    std::erase_if(owner->data.variants,[](const auto& entry){return entry.second.expired();});
    if(owner->data.variants.size()>=256)return {};
    auto candidate=std::make_shared<RootVariant>();
    if(FAILED(BuildPairedRoot(owner->data.bytes.data(),owner->data.bytes.size(),ordered,candidate->plan)))return {};
    probe::Scope internal;
    if(FAILED(device->CreateRootSignature(owner->data.node,candidate->plan.serialized.data(),candidate->plan.serialized.size(),IID_PPV_ARGS(&candidate->root))))return {};
    owner->data.variants.emplace(std::move(key),candidate);return candidate;
}
void ObserveNativeDepth(ID3D12PipelineState* original,const PassContract& pass,D3D12_SHADER_BYTECODE vs,D3D12_SHADER_BYTECODE ps,
    bool otherStages,bool streamOutput){
    if(!original)return;uint32_t status=2;
    try{
        if(!otherStages && !streamOutput && vs.pShaderBytecode && vs.BytecodeLength && !pass.targets.NumRenderTargets &&
            pass.depth==DXGI_FORMAT_D32_FLOAT_S8X24_UINT && pass.samples.Count==1 && !pass.samples.Quality &&
            pass.fixedRasterState && pass.depthStateKnown){
            const auto known=assets.find({Hash(vs.pShaderBytecode,vs.BytecodeLength),ps.BytecodeLength?Hash(ps.pShaderBytecode,ps.BytecodeLength):std::string{}});
            if(!ExplicitShaderResources(vs) || !ExplicitShaderResources(ps))status=6;
            else if(known!=assets.end())status=std::none_of(known->second.uses.begin(),known->second.uses.end(),
                [](const auto& use){return use.type==D3D12_DESCRIPTOR_RANGE_TYPE_UAV;})?1:3;
            else if(!ShaderReadOnly(vs))status=4;
            else status=ShaderReadOnly(ps)?1:5;
        }
    }catch(...){status=2;}
    original->SetPrivateData(nativeDepthKey,sizeof(status),&status);
    status=2;
    try{
        const auto count=pass.targets.NumRenderTargets;
        if(!otherStages && !streamOutput && (count==3 || count==4) && pass.targets.RTFormats[0]==DXGI_FORMAT_R10G10B10A2_UNORM &&
            pass.targets.RTFormats[1]==DXGI_FORMAT_R10G10B10A2_UNORM && pass.targets.RTFormats[2]==DXGI_FORMAT_R8G8B8A8_UNORM &&
            (count!=4 || pass.targets.RTFormats[3]==DXGI_FORMAT_R16G16_FLOAT) && pass.depth==DXGI_FORMAT_D32_FLOAT_S8X24_UINT &&
            pass.samples.Count==1 && !pass.samples.Quality && pass.fixedRasterState && pass.depthStateKnown){
            status=!ShaderSingleLayerReadOnly(vs,true)?3u:!ShaderSingleLayerReadOnly(ps,false)?4u:1u;
        }
    }catch(...){status=2;}
    original->SetPrivateData(nativeGeometryKey,sizeof(status),&status);
}
template<class Create> void Register(ID3D12Device* device,ID3D12RootSignature* root,ID3D12PipelineState* original,
    D3D12_SHADER_BYTECODE vs,D3D12_SHADER_BYTECODE ps,const D3D12_INPUT_LAYOUT_DESC& input,const PassContract& pass,Create create) {
    if(!device || !root || !original || !vs.pShaderBytecode || !vs.BytecodeLength || (ps.BytecodeLength && !ps.pShaderBytecode))return;
    Record record;record.id=nextId.fetch_add(1);record.original=reinterpret_cast<uintptr_t>(original);record.root=reinterpret_cast<uintptr_t>(root);record.result=E_FAIL;
    try {
        const auto key=std::make_pair(Hash(vs.pShaderBytecode,vs.BytecodeLength),ps.BytecodeLength?Hash(ps.pShaderBytecode,ps.BytecodeLength):std::string{});
        const auto found=assets.find(key);if(found==assets.end()){++CyberpunkVR_StereoFamilyMisses;return;}
        const auto& asset=found->second;Require(vs.BytecodeLength==asset.vsBytes && ps.BytecodeLength==asset.psBytes,"Native shader size differs");record.asset=asset.index;
        auto old=Lookup(original);if(old && old->sourceRoot.Get()==root && old->asset==asset.index){++CyberpunkVR_StereoFamilyHits;return;}
        auto family=std::make_shared<Family>();family->id=record.id;family->asset=asset.index;family->sourceRoot=root;family->uses=asset.uses;
        family->vertexSha256=key.first;family->pixelSha256=key.second;
        auto rootOwner=Private<RootOwner>(root,rootKey);Require(bool(rootOwner),"Missing original root identity");
        family->rootSha256=rootOwner->data.digest;
        family->instanceRebaseSafe=SupportsInstanceRebase(vs,input);
        family->instanceOrdinalIndependent=family->instanceRebaseSafe && HasIndependentInstanceOrdinal(vs);
        family->skinningReadContract=skinning::DepthContract(key.first,input);
        family->alphaDepthContract=alpha_depth::Contract(key.first,key.second,input);
        family->pass=pass;family->pass.pixelShader=ps.BytecodeLength!=0;
        family->pairedRoot=PairedRoot(device,root,asset);Require(bool(family->pairedRoot),"Missing or incompatible native root metadata");
        ComPtr<ID3D12Device2> device2;record.result=device->QueryInterface(IID_PPV_ARGS(&device2));
        if(SUCCEEDED(record.result)) {probe::Scope internal;record.result=create(device2.Get(),asset,family->pairedRoot->root.Get(),family->pipeline);}
        if(FAILED(record.result)){++CyberpunkVR_StereoFamilyFailures;Publish(record);return;}
        if(!asset.cameraVs.empty()){
            Asset camera=asset;camera.vs=asset.cameraVs;camera.ps=asset.cameraPs;
            probe::Scope internal;
            Require(SUCCEEDED(create(device2.Get(),camera,root,family->nativeCameraPipeline)),"Native camera pipeline failed");
            family->nativeCameraStages=asset.cameraStages;
        }
        record.pairedRoot=reinterpret_cast<uintptr_t>(family->pairedRoot->root.Get());record.pipeline=reinterpret_cast<uintptr_t>(family->pipeline.Get());record.pairs=UINT(asset.pairs.size());record.uses=UINT(asset.uses.size());
        ComPtr<FamilyOwner> owner;owner.Attach(new FamilyOwner);owner->data=family;
        record.result=E_PENDING;Publish(record,family.get());
        const auto attached=original->SetPrivateDataInterface(familyKey,owner.Get());
        if(FAILED(attached))++CyberpunkVR_StereoFamilyFailures;
        if(family->record<Capacity){std::lock_guard lock(recordsMutex);CyberpunkVR_StereoFamilySeq.fetch_add(1);
            CyberpunkVR_StereoFamilyRecords[family->record].result=attached;CyberpunkVR_StereoFamilySeq.fetch_add(1);}
    }catch(const std::exception&){++CyberpunkVR_StereoFamilyFailures;Publish(record);}
}
}
bool Enabled() {
    static const bool value=[]{wchar_t path[32768]{};const auto length=GetEnvironmentVariableW(L"CVR_STEREO_FAMILY_MANIFEST",path,32768);
        if(!length)return false;CyberpunkVR_StereoFamilyAssetStatus=1;
        try{Require(length<32768,"Family manifest path too long");Load(path);CyberpunkVR_StereoFamilyAssetStatus=2;return true;}
        catch(const std::exception& e){strncpy_s(CyberpunkVR_StereoFamilyError,e.what(),_TRUNCATE);CyberpunkVR_StereoFamilyAssetStatus=3;return false;}}();return value;
}
void RootCreated(ID3D12RootSignature* root,UINT node,const void* bytes,size_t size) {
    if(probe::internalCommands || !Enabled() || !root || !bytes || !size || size>1024*1024)return;
    try {
        auto existing=Private<RootOwner>(root,rootKey);if(existing)return;
        ComPtr<RootOwner> owner;owner.Attach(new RootOwner);owner->data.node=node;
        owner->data.bytes.assign(static_cast<const uint8_t*>(bytes),static_cast<const uint8_t*>(bytes)+size);owner->data.digest=Hash(bytes,size);
        if(FAILED(root->SetPrivateDataInterface(rootKey,owner.Get())))++CyberpunkVR_StereoFamilyFailures;
    }catch(...){++CyberpunkVR_StereoFamilyFailures;}
}
void GraphicsCreated(ID3D12Device* device,const D3D12_GRAPHICS_PIPELINE_STATE_DESC& source,ID3D12PipelineState* original) {
    if(probe::internalCommands || !Enabled())return;
    PassContract pass;pass.targets.NumRenderTargets=source.NumRenderTargets;std::copy_n(source.RTVFormats,8,pass.targets.RTFormats);
    pass.depth=source.DSVFormat;pass.samples=source.SampleDesc;pass.topology=source.PrimitiveTopologyType;
    pass.depthState=source.DepthStencilState;pass.depthStateKnown=true;
    pass.fixedRasterState=source.Flags==D3D12_PIPELINE_STATE_FLAG_NONE && !source.RasterizerState.ForcedSampleCount;
    const bool other=source.GS.BytecodeLength || source.HS.BytecodeLength || source.DS.BytecodeLength;
    const bool stream=source.StreamOutput.NumEntries || source.StreamOutput.NumStrides;
    ObserveNativeDepth(original,pass,source.VS,source.PS,other,stream);if(other || stream)return;
    Register(device,source.pRootSignature,original,source.VS,source.PS,source.InputLayout,pass,[&](ID3D12Device2* d,const Asset& a,ID3D12RootSignature* root,ComPtr<ID3D12PipelineState>& pso){
        auto desc=source;desc.pRootSignature=root;desc.VS={a.vs.data(),a.vs.size()};desc.PS=a.ps.empty()?D3D12_SHADER_BYTECODE{}:D3D12_SHADER_BYTECODE{a.ps.data(),a.ps.size()};
        ViewInstancedLayout layout;layout.allowMasking=true;return CreateViewInstancedPipeline(d,desc,layout,&pso);
    });
}
void StreamCreated(ID3D12Device* device,const D3D12_PIPELINE_STATE_STREAM_DESC& source,ID3D12PipelineState* original) {
    if(probe::internalCommands || !Enabled())return;PipelineStreamInfo info;
    if(!ReadPipelineStream(source,info))return;
    PassContract pass{info.targets,info.depthFormat,info.samples,info.topology,
        !info.depthBounds && !info.forcedSamples && info.flags==D3D12_PIPELINE_STATE_FLAG_NONE};
    pass.depthState=info.depth;pass.depthStateKnown=info.haveDepth;
    ObserveNativeDepth(original,pass,info.vs,info.ps,info.otherStages || info.viewInstancing,info.streamOutput);
    if(info.otherStages || info.viewInstancing || info.streamOutput)return;
    Register(device,info.root,original,info.vs,info.ps,info.input,pass,[&](ID3D12Device2* d,const Asset& a,ID3D12RootSignature* root,ComPtr<ID3D12PipelineState>& pso){
        ViewInstancedLayout layout;layout.allowMasking=true;return CreateViewInstancedPipelineStream(d,source,{a.vs.data(),a.vs.size()},layout,&pso,
            a.ps.empty()?D3D12_SHADER_BYTECODE{}:D3D12_SHADER_BYTECODE{a.ps.data(),a.ps.size()},root);
    });
}
std::shared_ptr<const Family> Lookup(ID3D12PipelineState* pso) {auto owner=Private<FamilyOwner>(pso,familyKey);return owner?owner->data:nullptr;}
bool NativeDepthCompatible(ID3D12PipelineState* pso){
    return NativeDepthStatus(pso)==1;
}
uint32_t NativeDepthStatus(ID3D12PipelineState* pso){
    uint32_t status{};UINT bytes=sizeof(status);return pso && SUCCEEDED(pso->GetPrivateData(nativeDepthKey,&bytes,&status)) && bytes==sizeof(status)?status:0;
}
uint32_t NativeGeometryStatus(ID3D12PipelineState* pso){
    uint32_t status{};UINT bytes=sizeof(status);
    return pso && SUCCEEDED(pso->GetPrivateData(nativeGeometryKey,&bytes,&status)) && bytes==sizeof(status)?status:0;
}
void PipelineBound(ID3D12GraphicsCommandList* list,ID3D12PipelineState* pso) {
    if(probe::internalCommands || !Enabled() || !list)return;
    observation_profile::Scope profile(observation_profile::Phase::BindPipeline);
    auto owner=Private<FamilyOwner>(pso,familyKey);if(FAILED(list->SetPrivateDataInterface(boundFamilyKey,owner.Get())))++CyberpunkVR_StereoFamilyBindingFailures;
}
void RootBound(ID3D12GraphicsCommandList* list,ID3D12RootSignature* root) {
    if(probe::internalCommands || !Enabled() || !list)return;observation_profile::Scope profile(observation_profile::Phase::BindRoot);
    if(FAILED(list->SetPrivateDataInterface(boundRootKey,root)))++CyberpunkVR_StereoFamilyBindingFailures;
}
void Reset(ID3D12GraphicsCommandList* list,ID3D12PipelineState* pso){PipelineBound(list,pso);RootBound(list,nullptr);}
std::shared_ptr<const Family> Bound(ID3D12GraphicsCommandList* list) {
    if(!list || CyberpunkVR_StereoFamilyBindingFailures.load(std::memory_order_relaxed))return {};
    auto owner=Private<FamilyOwner>(list,boundFamilyKey);if(!owner)return {};
    auto root=Private<ID3D12RootSignature>(list,boundRootKey);const auto& family=owner->data;
    if(root.Get()!=family->sourceRoot.Get()) {
        auto actual=Private<RootOwner>(root.Get(),rootKey),expected=Private<RootOwner>(family->sourceRoot.Get(),rootKey);
        if(!actual || !expected || actual->data.digest!=expected->data.digest)return {};
    }
    return family;
}
void ObserveDraw(ID3D12GraphicsCommandList* list,UINT node,int side) {
    if(!CyberpunkVR_StereoFamilyObserve.load(std::memory_order_relaxed) || CyberpunkVR_StereoFamilyBindingFailures.load(std::memory_order_relaxed) ||
       probe::internalCommands || node!=0x33DDA4 || side<0 || side>1 || !Enabled())return;
    auto owner=Private<FamilyOwner>(list,boundFamilyKey);if(!owner){++CyberpunkVR_StereoFamilyUnmatched[side];return;}
    auto root=Private<ID3D12RootSignature>(list,boundRootKey);const auto& family=owner->data;
    if(root.Get()!=family->sourceRoot.Get()) {
        auto actual=Private<RootOwner>(root.Get(),rootKey),expected=Private<RootOwner>(family->sourceRoot.Get(),rootKey);
        if(!actual || !expected || actual->data.digest!=expected->data.digest){++CyberpunkVR_StereoFamilyRootMismatch[side];return;}
    }
    if(family->record>=Capacity)return;
    std::lock_guard lock(recordsMutex);CyberpunkVR_StereoFamilySeq.fetch_add(1);
    ++CyberpunkVR_StereoFamilyRecords[family->record].draws[side];CyberpunkVR_StereoFamilySeq.fetch_add(1);
}
}
#endif
