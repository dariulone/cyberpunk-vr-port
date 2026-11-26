#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Render/MonoCascades.hpp"
#include "Render/MonoCascadePolicy.hpp"
#include "Render/MonoCascadeImage.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/CommandResources.hpp"
#include "Stereo/EngineStereo.hpp"
#include <wrl/client.h>
#include <array>
#include <bit>
#include <cstring>
#include <mutex>
#include <unordered_map>
#include <vector>
#include <cstdio>
#include <memory>
#if CVR_RUST_VRIK
// The exports' tables are attached by rust/crates/cvr-render/src/export_attach.rs as the plugin loads;
// nothing of this file remains in the Rust build.
#else

extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_MonoCascades{1};
__declspec(dllexport) uint32_t CyberpunkVR_MonoCascadeReportSeq=0;
__declspec(dllexport) char CyberpunkVR_MonoCascadeReport[4096]="{}";
}
namespace cvr::cascades {
using Microsoft::WRL::ComPtr;
namespace {
constexpr GUID signatureKey{0xc2e74a33,0x985a,0x414b,{0xa6,0xdd,0x7c,0xcf,0x0e,0xbe,0x64,0x09}};
constexpr GUID allocationKey{0x1bbcdd19,0xf3c8,0x492b,{0x9a,0xe1,0xd7,0x44,0x40,0x18,0x7f,0x6d}};
struct View {Atlas atlas;DXGI_FORMAT format{};};
std::mutex mutex;
std::atomic<bool> activePair{};
std::atomic<bool> snapshotReady{},haveRecordings{};
std::unordered_map<SIZE_T,View> descriptors;
Policy policy;
Atlas mainAtlas;
ComPtr<ID3D12Resource> source;
Allocation sourceAllocation;
std::shared_ptr<AtlasImage> atlasImage;
uint32_t copiedMask{};
uint64_t capturedSlices{},restoredAtlases{},captureFailures{};
struct Recording {
    uint64_t cycle{};
    ComPtr<ID3D12Resource> source;
    std::shared_ptr<AtlasImage> image;
    uint32_t written{},copied{};
    std::array<D3D12_RESOURCE_STATES,8> states{};
    std::array<bool,8> split{};
};
std::unordered_map<ID3D12GraphicsCommandList*,std::shared_ptr<Recording>> recordings;
std::array<float,64> sampling{};
std::array<std::array<uint8_t,848>,4> cameras{};
std::array<bool,4> haveCamera{};
uint64_t lastReport{},sourceDraws{},mainDraws{},skipped{},clearSkips{},fallbacks{},unknownDsv{},discards{},invalidWrites{};
uint64_t badSignatures{},badBindings{},badCameras{};
uint64_t copyInvalidations{},discardInvalidations{},aliasInvalidations{},discardSkips{};
struct Node {
    bool active{},source{},valid{},bound{},hadBinding{},reuse{},cameraKnown{},repaired{};
    unsigned index{};uint32_t clearBits{};uint64_t cycle{},draws{},badSignatures{},badBindings{};
    ID3D12GraphicsCommandList* list{};
    View view{};
    Atlas expected{};
    D3D12_CPU_DESCRIPTOR_HANDLE dsv{};
    std::shared_ptr<Recording> recording;
};
thread_local std::vector<Node> stack;
thread_local Node node;
thread_local bool pendingRepair{};
thread_local unsigned pendingIndex{};
thread_local uint64_t pendingCycle{};
// Off in engine stereo, the descriptor and allocation tracking too: the engine draws the cascades once for both eyes,
// so there is no second view to lend them to. Left on, every MAIN cascade node waited for a snapshot that never came
// and was cleared a second time, and every discard took the lock.
bool SecondView(){static const bool wanted=engine_stereo::SecondViewWanted();return wanted;}
bool Enabled(){
    return SecondView() && (activePair.load(std::memory_order_relaxed) || CyberpunkVR_MonoCascades.load(std::memory_order_relaxed)!=0) &&
        !stereo::probe::internalCommands;
}
View Lookup(D3D12_CPU_DESCRIPTOR_HANDLE handle){const auto it=descriptors.find(handle.ptr);return it==descriptors.end()?View{}:it->second;}
Allocation Memory(ID3D12Resource* resource){Allocation value;UINT bytes=sizeof(value);if(!resource || FAILED(resource->GetPrivateData(allocationKey,&bytes,&value)) || bytes!=sizeof(value))return {};return value;}
bool BackupReady(){return policy.active && atlasImage && copiedMask==((1u<<policy.active)-1);}
void Capture(ID3D12GraphicsCommandList* list,Recording& r,uint32_t mask) {
    if(r.cycle!=policy.cycle)return;
    for(unsigned i=0;i<8;++i)if((mask&r.written&~r.copied)&(1u<<i)) {
        if(r.split[i] || !r.image->Capture(list,r.source.Get(),i,r.states[i])){policy.Invalidate();++captureFailures;continue;}
        r.copied|=1u<<i;copiedMask|=1u<<i;++capturedSlices;snapshotReady.store(BackupReady(),std::memory_order_release);
    }
}
}
void ResourceCreated(ID3D12Resource* resource,ID3D12Heap* heap,uint64_t offset,uint64_t bytes) {
    if(!resource || stereo::probe::internalCommands || !SecondView())return;
    const Allocation memory{reinterpret_cast<uintptr_t>(heap),offset,bytes,!heap || (bytes && bytes!=UINT64_MAX),heap==nullptr};
    resource->SetPrivateData(allocationKey,sizeof(memory),&memory);
}
void DsvCreated(ID3D12Resource* resource,const D3D12_DEPTH_STENCIL_VIEW_DESC* desc,D3D12_CPU_DESCRIPTOR_HANDLE handle) {
    if(stereo::probe::internalCommands || !SecondView())return;
    View view;
    if(resource) {
        const auto d=resource->GetDesc();const auto format=desc?desc->Format:d.Format;
        if(d.Dimension==D3D12_RESOURCE_DIMENSION_TEXTURE2D && d.SampleDesc.Count==1 &&
           (format==DXGI_FORMAT_D16_UNORM || format==DXGI_FORMAT_D32_FLOAT) && (!desc || desc->Flags==D3D12_DSV_FLAG_NONE)) {
            view.atlas={reinterpret_cast<uintptr_t>(resource),uint32_t(d.Width),d.Height,uint32_t(d.Format),d.DepthOrArraySize,d.MipLevels,0,0,1};view.format=format;
            if(desc && desc->ViewDimension==D3D12_DSV_DIMENSION_TEXTURE2DARRAY) {
                view.atlas.mip=desc->Texture2DArray.MipSlice;view.atlas.first=desc->Texture2DArray.FirstArraySlice;view.atlas.count=desc->Texture2DArray.ArraySize;
            }else if(desc && desc->ViewDimension==D3D12_DSV_DIMENSION_TEXTURE2D)view.atlas.mip=desc->Texture2D.MipSlice;
            else if(desc)view={};
        }
    }
    std::lock_guard lock(mutex);
    if(descriptors.size()>=32768 && !descriptors.contains(handle.ptr))descriptors.clear();
    if(view.atlas.Valid())descriptors[handle.ptr]=view;else descriptors.erase(handle.ptr);
}
void CopyDescriptors(ID3D12Device* device,UINT destinations,const D3D12_CPU_DESCRIPTOR_HANDLE* dest,const UINT* sizes,UINT sources,const D3D12_CPU_DESCRIPTOR_HANDLE* src,const UINT* counts,D3D12_DESCRIPTOR_HEAP_TYPE type) {
    if(type!=D3D12_DESCRIPTOR_HEAP_TYPE_DSV || !device || !dest || !src || stereo::probe::internalCommands || !SecondView())return;
    const auto stride=device->GetDescriptorHandleIncrementSize(type);
    std::lock_guard lock(mutex);std::vector<View> copy;
    for(UINT i=0;i<sources;++i)for(UINT n=0;n<(counts?counts[i]:1);++n){if(copy.size()>=32768){descriptors.clear();return;}copy.push_back(Lookup({src[i].ptr+size_t(n)*stride}));}
    size_t at=0;
    for(UINT i=0;i<destinations;++i)for(UINT n=0;n<(sizes?sizes[i]:1);++n) {
        const auto handle=dest[i].ptr+size_t(n)*stride;
        if(at<copy.size() && copy[at].atlas.Valid())descriptors[handle]=copy[at];else descriptors.erase(handle);++at;
    }
}
void SourceSampling(const float* matrices,unsigned count) {
    if(!Enabled() || !matrices)return;
    std::lock_guard lock(mutex);const bool enabled=CyberpunkVR_MonoCascades.load(std::memory_order_relaxed)!=0;
    activePair.store(enabled,std::memory_order_relaxed);policy.Begin(enabled?count:0);mainAtlas={};copiedMask=0;snapshotReady.store(false,std::memory_order_release);std::memcpy(sampling.data(),matrices,sizeof(sampling));haveCamera={};source.Reset();
}
void MainSampling(const float* matrices,unsigned count) {
    if(!Enabled())return;
    std::lock_guard lock(mutex);policy.MainSampling(count,matrices && std::memcmp(matrices,sampling.data(),sizeof(sampling))==0);
}
void BeginNode(int side,int index) {
    stack.push_back(node);node={};
    if(!Enabled() || side<0 || side>1 || index<0 || index>=4)return;
    std::lock_guard lock(mutex);node.active=true;node.source=side==1;node.index=unsigned(index);node.cycle=policy.cycle;
    node.valid=policy.clear && !policy.invalid && node.index<policy.active;
    node.reuse=!node.source && pendingRepair && pendingCycle==policy.cycle && pendingIndex==node.index;pendingRepair=false;
    node.expected=node.source?policy.atlas:mainAtlas;
    node.clearBits=policy.clearBits;
}
bool SkipNode(int side,int index) {
    pendingRepair=false;
    if(!Enabled() || side!=0 || index<0 || index>=4)return false;
    std::lock_guard lock(mutex);
    if(!policy.Claim(unsigned(index)))return false;
    if(policy.invalid){pendingRepair=true;pendingCycle=policy.cycle;pendingIndex=unsigned(index);return false;}
    ++skipped;return true;
}
void EndNode() {
    if(node.active) {
        std::lock_guard lock(mutex);
        if(node.source) {sourceDraws+=node.draws;badSignatures+=node.badSignatures;badBindings+=node.badBindings;if(node.cycle==policy.cycle)policy.Complete(node.index,node.valid && node.hadBinding && node.cameraKnown);}
        else mainDraws+=node.draws;
    }
    node=stack.back();stack.pop_back();
}
void Camera(unsigned index,const void* data,size_t bytes,bool sourceSide) {
    if(!Enabled() || !node.active || node.index!=index || sourceSide!=node.source || bytes!=848 || !data)return;
    std::lock_guard lock(mutex);if(node.cycle!=policy.cycle){node.valid=false;return;}
    if(sourceSide) {
        const bool same=!haveCamera[index] || std::memcmp(cameras[index].data(),data,bytes)==0;
        if(!same)++badCameras;
        if(!haveCamera[index]){std::memcpy(cameras[index].data(),data,bytes);haveCamera[index]=true;}
        node.cameraKnown=same;node.valid&=same;policy.Camera(index,same);
    }else node.cameraKnown=haveCamera[index] && std::memcmp(cameras[index].data(),data,bytes)==0;
}
void Targets(ID3D12GraphicsCommandList* list,UINT colors,const D3D12_CPU_DESCRIPTOR_HANDLE* dsv) {
    if(!Enabled() || !node.active)return;
    std::lock_guard lock(mutex);node.list=list;node.view=dsv?Lookup(*dsv):View{};node.dsv=dsv?*dsv:D3D12_CPU_DESCRIPTOR_HANDLE{};
    node.bound=colors==0 && node.view.atlas.Valid() && node.expected.SameResource(node.view.atlas) && node.view.atlas.Mask()==(1u<<node.index);
    node.hadBinding|=node.bound;if(dsv && !node.view.atlas.Valid())++unknownDsv;
    if(node.source && node.bound && atlasImage) {
        auto& r=recordings[list];if(!r || r->cycle!=policy.cycle){r=std::make_shared<Recording>();r->cycle=policy.cycle;r->source=source;r->image=atlasImage;}
        haveRecordings.store(true,std::memory_order_release);
        node.recording=r;
    }
}
bool SkipDraw(ID3D12GraphicsCommandList* list) {
    if(!Enabled() || !node.active)return false;
    ++node.draws;
    if(node.source){const bool bound=node.bound && node.list==list && node.recording;node.valid&=bound;if(!bound)++node.badBindings;
        if(bound){auto& r=*node.recording;const auto bit=1u<<node.index;r.written|=bit;r.states[node.index]=D3D12_RESOURCE_STATE_DEPTH_WRITE;
            if(r.copied&bit){std::lock_guard lock(mutex);r.copied&=~bit;if(r.cycle==policy.cycle){copiedMask&=~bit;snapshotReady.store(false,std::memory_order_release);}}}
        return false;}
    // Reuse skips whole MAIN nodes, never a prefix of their draw sequence.
    // A rejected node gets its native clear back before its first native draw.
    if(node.reuse && !node.repaired && node.bound && node.list==list) {
        stereo::probe::Scope internal;
        const float depth=std::bit_cast<float>(node.clearBits);
        {std::lock_guard lock(mutex);++fallbacks;}
        list->ClearDepthStencilView(node.dsv,D3D12_CLEAR_FLAG_DEPTH,depth,0,0,nullptr);node.repaired=true;node.reuse=false;
    }
    return false;
}
void SignatureCreated(ID3D12CommandSignature* signature,const D3D12_COMMAND_SIGNATURE_DESC& desc) {
    if(!signature)return;
    const uint32_t pure=desc.NumArgumentDescs==1 && desc.pArgumentDescs &&
        (desc.pArgumentDescs[0].Type==D3D12_INDIRECT_ARGUMENT_TYPE_DRAW || desc.pArgumentDescs[0].Type==D3D12_INDIRECT_ARGUMENT_TYPE_DRAW_INDEXED);
    signature->SetPrivateData(signatureKey,sizeof(pure),&pure);
}
bool SkipIndirect(ID3D12GraphicsCommandList* list,ID3D12CommandSignature* signature) {
    if(!Enabled() || !node.active)return false;
    uint32_t pure=0;UINT bytes=sizeof(pure);
    if(!signature || FAILED(signature->GetPrivateData(signatureKey,&bytes,&pure)) || bytes!=sizeof(pure) || !pure) {
        node.valid=false;++node.badSignatures;return SkipDraw(list);
    }
    return SkipDraw(list);
}
bool Clear(ID3D12GraphicsCommandList* list,D3D12_CPU_DESCRIPTOR_HANDLE dsv,D3D12_CLEAR_FLAGS flags,FLOAT depth,UINT8 stencil,UINT rects,int side,bool clearNode) {
    if(!Enabled() || !clearNode || side<0 || side>1)return false;
    std::lock_guard lock(mutex);const auto view=Lookup(dsv);
    if(flags!=D3D12_CLEAR_FLAG_DEPTH || stencil || rects || !view.atlas.Valid()){policy.Invalidate();++fallbacks;return false;}
    if(side==1) {
        if(policy.SourceClear(view.atlas,std::bit_cast<uint32_t>(depth))) {
            source=reinterpret_cast<ID3D12Resource*>(view.atlas.resource);sourceAllocation=Memory(source.Get());
            if(!atlasImage || !atlasImage->Compatible(source.Get())) {
                ComPtr<ID3D12Device> device;auto image=std::make_shared<AtlasImage>();
                if(FAILED(list->GetDevice(IID_PPV_ARGS(&device))) || !image->Initialize(device.Get(),source.Get(),view.format)){policy.Invalidate();++captureFailures;return false;}
                atlasImage=std::move(image);
            }
            if(!atlasImage->Clear(list,depth)){policy.Invalidate();++captureFailures;}
        }
        return false;
    }
    mainAtlas=view.atlas;
    if(!BackupReady())policy.Invalidate();
    if(policy.MainClear(view.atlas,std::bit_cast<uint32_t>(depth))) {
        if(atlasImage->Restore(list,reinterpret_cast<ID3D12Resource*>(view.atlas.resource),view.atlas.first,view.atlas.count)){++clearSkips;++restoredAtlases;return true;}
        policy.Invalidate();policy.reusing=false;++captureFailures;
    }
    ++fallbacks;return false;
}
void Write(ID3D12Resource* resource) {
    if(!Enabled() || !resource || snapshotReady.load(std::memory_order_acquire))return;
    std::lock_guard lock(mutex);if(policy.atlas.resource==reinterpret_cast<uintptr_t>(resource) && !BackupReady()){policy.Invalidate();++invalidWrites;++copyInvalidations;}
}
bool Discard(ID3D12Resource* resource,int,bool){
    if(!Enabled() || snapshotReady.load(std::memory_order_acquire))return false;
    std::lock_guard lock(mutex);++discards;
    if(policy.atlas.resource!=reinterpret_cast<uintptr_t>(resource))return false;
    if(!BackupReady()){policy.Invalidate();++invalidWrites;++discardInvalidations;}return false;
}
void Barriers(UINT count,const D3D12_RESOURCE_BARRIER* barriers) {
    if(!Enabled() || !barriers || snapshotReady.load(std::memory_order_acquire))return;
    for(UINT i=0;i<count;++i)if(barriers[i].Type==D3D12_RESOURCE_BARRIER_TYPE_ALIASING) {
        std::lock_guard lock(mutex);const auto& a=barriers[i].Aliasing;
        if(!policy.clear)continue;
        const bool explicitAtlas=reinterpret_cast<uintptr_t>(a.pResourceBefore)==policy.atlas.resource || reinterpret_cast<uintptr_t>(a.pResourceAfter)==policy.atlas.resource;
        const bool overlap=(!a.pResourceBefore && !a.pResourceAfter)?!(sourceAllocation.known && sourceAllocation.committed):
            ((a.pResourceBefore && sourceAllocation.CouldAlias(Memory(a.pResourceBefore))) || (a.pResourceAfter && sourceAllocation.CouldAlias(Memory(a.pResourceAfter))));
        // The native heap can be recycled while other cascades are recording.
        // Each producer preserves its own slice before relinquishing ownership.
        if(explicitAtlas || overlap)++aliasInvalidations;
    }
}
void BeforeBarriers(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RESOURCE_BARRIER* barriers) {
    if(!Enabled() || !barriers || !haveRecordings.load(std::memory_order_acquire))return;
    std::lock_guard lock(mutex);const auto it=recordings.find(list);if(it==recordings.end())return;auto& r=*it->second;
    for(UINT n=0;n<count;++n){const auto& b=barriers[n];
        if(b.Type==D3D12_RESOURCE_BARRIER_TYPE_ALIASING){Capture(list,r,255);continue;}
        if(b.Type!=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION || b.Transition.pResource!=r.source.Get())continue;
        const auto& t=b.Transition;const uint32_t mask=t.Subresource==D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES?255:(t.Subresource<8?1u<<t.Subresource:0);
        if(b.Flags!=D3D12_RESOURCE_BARRIER_FLAG_END_ONLY && (t.StateBefore&D3D12_RESOURCE_STATE_DEPTH_WRITE) && !(t.StateAfter&D3D12_RESOURCE_STATE_DEPTH_WRITE))Capture(list,r,mask);
        for(unsigned i=0;i<8;++i)if(mask&(1u<<i)){r.split[i]=b.Flags==D3D12_RESOURCE_BARRIER_FLAG_BEGIN_ONLY;if(!r.split[i])r.states[i]=t.StateAfter;}
    }
}
void Close(ID3D12GraphicsCommandList* list){if(!Enabled() || !haveRecordings.load(std::memory_order_acquire))return;std::lock_guard lock(mutex);const auto it=recordings.find(list);
    if(it!=recordings.end()){Capture(list,*it->second,255);recordings.erase(it);haveRecordings.store(!recordings.empty(),std::memory_order_release);}}
void Reset(ID3D12GraphicsCommandList* list){if(stereo::probe::internalCommands || !haveRecordings.load(std::memory_order_acquire))return;std::lock_guard lock(mutex);recordings.erase(list);haveRecordings.store(!recordings.empty(),std::memory_order_release);}
void Poll() {
    if(!SecondView())return;
    std::lock_guard lock(mutex);const auto now=GetTickCount64();if(now-lastReport<250)return;lastReport=now;
    InterlockedIncrement(reinterpret_cast<volatile LONG*>(&CyberpunkVR_MonoCascadeReportSeq));
    std::snprintf(CyberpunkVR_MonoCascadeReport,sizeof(CyberpunkVR_MonoCascadeReport),
        "{\"mode\":%u,\"cycle\":%llu,\"active\":%u,\"width\":%u,\"height\":%u,\"slices\":%u,\"complete\":%u,\"camera\":%u,\"main_sampling\":%d,\"reusing\":%d,\"invalid\":%d,\"source_draws\":%llu,\"main_draws\":%llu,\"skipped_nodes\":%llu,\"clear_skips\":%llu,\"fallbacks\":%llu,\"unknown_dsv\":%llu,\"discards\":%llu,\"invalid_writes\":%llu,\"allocation_known\":%d,\"committed\":%d,\"bad_signatures\":%llu,\"bad_bindings\":%llu,\"bad_cameras\":%llu,\"copy_invalidations\":%llu,\"discard_invalidations\":%llu,\"alias_invalidations\":%llu,\"copied_mask\":%u,\"captured_slices\":%llu,\"restored_atlases\":%llu,\"capture_failures\":%llu}",
        CyberpunkVR_MonoCascades.load(),policy.cycle,policy.active,policy.atlas.width,policy.atlas.height,policy.atlas.slices,policy.complete,policy.camera,
        int(policy.mainSampling),int(policy.reusing),int(policy.invalid),sourceDraws,mainDraws,skipped,clearSkips,fallbacks,unknownDsv,discards,invalidWrites,int(sourceAllocation.known),int(sourceAllocation.committed),badSignatures,badBindings,badCameras,copyInvalidations,discardInvalidations,aliasInvalidations,copiedMask,capturedSlices,restoredAtlases,captureFailures);
    InterlockedIncrement(reinterpret_cast<volatile LONG*>(&CyberpunkVR_MonoCascadeReportSeq));
}
}
#endif
#endif
