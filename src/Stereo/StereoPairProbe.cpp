// StereoPairProbe -- see include/Stereo/StereoPairProbe.hpp.
#include "Stereo/StereoPairProbe.hpp"
#include "Core/Paths.hpp"
#include <windows.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <cstring>
#include <memory>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>

extern void Log(const char*,...);
namespace cvr::rootsig { bool Blob(ID3D12RootSignature*,std::vector<uint8_t>&); }   // Stereo/DeviceHooks.cpp

namespace {
constexpr uint32_t kRecordParams=40;
struct Record {
    uint64_t list,pso,root,ib,vb;
    uint64_t params[kRecordParams];
    uint8_t types[kRecordParams];   // per parameter: 0 unset, 1 table, 2 CBV, 3 SRV, 4 UAV, 5 constants
    uint32_t node,serial,kind;
    int32_t viewportX;
    uint32_t viewportWidth;
    uint32_t args[5];
    uint32_t sequence;        // the draw's place in its list
    uint32_t pad;
};
static_assert(sizeof(Record)==448);
constexpr uint32_t kCapacity=16384;
}
extern "C" {
__declspec(dllexport) uint32_t CyberpunkVR_PairProbeRecordBytes=sizeof(Record);
// Frames to record (1..8); 0 when taken. State: 0 idle, 1 armed, 2 recording, 3 done, 4 not installed.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PairProbeRequest{0},CyberpunkVR_PairProbeState{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PairProbeNode{0x33DDA4};   // RenderElements; 0 = every node
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PairProbeCount{0};
__declspec(dllexport) Record CyberpunkVR_PairProbeRecords[kCapacity]{};
// Single-pass feasibility (StereoPairProbe.hpp): root parameters (bit n = parameter n) whose tables a right-eye draw
// takes from its left partner; 0 = off. 1 = also the partner's start instance.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PairSubstituteMask{0},CyberpunkVR_PairSubstituteMaskHigh{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PairSubstituteInstance{0};
// [0] right-eye draws substituted, [1] right-eye draws without a matching partner, [2] left-eye draws kept,
// [3] tables substituted.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PairSubstituteCounts[4]{};
extern std::atomic<uint64_t> CyberpunkVR_PresentTiming[8];
}

namespace cvr::stereo::pair_probe {
namespace {
using RootFn=void(STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,UINT,UINT64);
using SigFn=void(STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,ID3D12RootSignature*);
using ConstFn=void(STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,UINT,UINT,UINT);
using ConstsFn=void(STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,UINT,UINT,const void*,UINT);
using IbFn=void(STDMETHODCALLTYPE*)(ID3D12GraphicsCommandList*,const D3D12_INDEX_BUFFER_VIEW*);
struct Originals {
    void** vtable{};
    SigFn signature{};
    RootFn table{},cbv{},srv{},uav{};
    ConstFn constant{};
    ConstsFn constants{};
    IbFn ib{};
};
std::array<Originals,16> originals;
std::atomic<uint32_t> originalCount{0};
std::mutex installMutex;
const Originals* OriginalsOf(ID3D12GraphicsCommandList* list) {
    void** vtable=*reinterpret_cast<void***>(list);
    const auto n=originalCount.load(std::memory_order_acquire);
    for(uint32_t i=0;i<n;++i)if(originals[i].vtable==vtable)return &originals[i];
    return nullptr;
}
constexpr uint32_t kParams=64;
struct Partner {uint64_t pso,ib,vb;uint32_t kind,startInstance;uint64_t tables[kParams];uint64_t tableMask;};
struct State {
    ID3D12RootSignature* root{};
    uint64_t params[kRecordParams]{};
    uint8_t types[kRecordParams]{};
    uint64_t tables[kParams]{};     // every table parameter's handle as the engine bound it
    uint64_t tableMask{};           // which of them are tables
    std::vector<Partner> left;      // the left batch just recorded
    uint32_t rightIndex{};
    int lastEye{-1};                // 0 left, 1 right, -1 other
    uint64_t restoreMask{};         // tables AfterDraw must put back
    uint32_t constants[kRecordParams][64]{};
    uint64_t ib{},vb{};
    int32_t viewportX{-1};uint32_t viewportWidth{};
    uint32_t sequence{};
};
std::mutex stateMutex;
std::unordered_map<ID3D12GraphicsCommandList*,std::unique_ptr<State>> states;
thread_local ID3D12GraphicsCommandList* t_list{};
thread_local State* t_state{};
State& StateOf(ID3D12GraphicsCommandList* list) {
    if(t_list==list && t_state)return *t_state;
    std::lock_guard lock(stateMutex);
    auto& slot=states[list];
    if(!slot)slot=std::make_unique<State>();
    t_list=list;t_state=slot.get();
    return *slot;
}
bool wanted{},checked{};
std::atomic<bool> tracking{false};
uint64_t windowStart{},windowEnd{};
uint64_t Fnv(const void* data,size_t bytes,uint64_t h=1469598103934665603ull) {
    for(size_t i=0;i<bytes;++i){h^=static_cast<const uint8_t*>(data)[i];h*=1099511628211ull;}
    return h;
}
void SetParam(ID3D12GraphicsCommandList* list,UINT index,uint64_t value,uint64_t type) {
    auto& s=StateOf(list);
    if(index<kParams) {
        if(type==1){s.tables[index]=value;s.tableMask|=1ull<<index;}
        else s.tableMask&=~(1ull<<index);
    }
    if(index>=kRecordParams)return;
    s.params[index]=value;
    s.types[index]=uint8_t(type);
}
void STDMETHODCALLTYPE Signature(ID3D12GraphicsCommandList* list,ID3D12RootSignature* root) {
    const auto* o=OriginalsOf(list);if(o && o->signature)o->signature(list,root);
    auto& s=StateOf(list);s.root=root;std::memset(s.params,0,sizeof(s.params));std::memset(s.types,0,sizeof(s.types));
    std::memset(s.tables,0,sizeof(s.tables));s.tableMask=0;
}
void STDMETHODCALLTYPE Table(ID3D12GraphicsCommandList* list,UINT index,UINT64 handle) {
    const auto* o=OriginalsOf(list);if(o && o->table)o->table(list,index,handle);
    SetParam(list,index,handle,1);
}
void STDMETHODCALLTYPE Cbv(ID3D12GraphicsCommandList* list,UINT index,UINT64 address) {
    const auto* o=OriginalsOf(list);if(o && o->cbv)o->cbv(list,index,address);
    SetParam(list,index,address,2);
}
void STDMETHODCALLTYPE Srv(ID3D12GraphicsCommandList* list,UINT index,UINT64 address) {
    const auto* o=OriginalsOf(list);if(o && o->srv)o->srv(list,index,address);
    SetParam(list,index,address,3);
}
void STDMETHODCALLTYPE Uav(ID3D12GraphicsCommandList* list,UINT index,UINT64 address) {
    const auto* o=OriginalsOf(list);if(o && o->uav)o->uav(list,index,address);
    SetParam(list,index,address,4);
}
void ConstantsInto(ID3D12GraphicsCommandList* list,UINT index,UINT count,const uint32_t* values,UINT offset) {
    if(index>=kRecordParams)return;
    auto& s=StateOf(list);
    for(UINT i=0;i<count && offset+i<64;++i)s.constants[index][offset+i]=values[i];
    SetParam(list,index,Fnv(s.constants[index],sizeof(s.constants[index])),5);
}
void STDMETHODCALLTYPE Constant(ID3D12GraphicsCommandList* list,UINT index,UINT value,UINT offset) {
    const auto* o=OriginalsOf(list);if(o && o->constant)o->constant(list,index,value,offset);
    ConstantsInto(list,index,1,&value,offset);
}
void STDMETHODCALLTYPE Constants(ID3D12GraphicsCommandList* list,UINT index,UINT count,const void* values,UINT offset) {
    const auto* o=OriginalsOf(list);if(o && o->constants)o->constants(list,index,count,values,offset);
    if(values)ConstantsInto(list,index,count,static_cast<const uint32_t*>(values),offset);
}
void STDMETHODCALLTYPE IndexBuffer(ID3D12GraphicsCommandList* list,const D3D12_INDEX_BUFFER_VIEW* view) {
    const auto* o=OriginalsOf(list);if(o && o->ib)o->ib(list,view);
    StateOf(list).ib=view ? Fnv(view,sizeof(*view)) : 0;
}

// The layout of each root signature the probed node draws with, once, into the plugin's log: which registers each
// table parameter holds (what a view-instanced twin has to provide per view).
void DescribeRoot(ID3D12RootSignature* root) {
    static std::mutex mutex;
    static std::vector<ID3D12RootSignature*> described;
    if(!root)return;
    {
        std::lock_guard lock(mutex);
        if(described.size()>=8 || std::find(described.begin(),described.end(),root)!=described.end())return;
        described.push_back(root);
    }
    std::vector<uint8_t> blob;
    if(!cvr::rootsig::Blob(root,blob)){Log("[pair-probe] root %p: blob not kept\n",static_cast<void*>(root));return;}
    ID3D12VersionedRootSignatureDeserializer* des=nullptr;
    if(FAILED(D3D12CreateVersionedRootSignatureDeserializer(blob.data(),blob.size(),IID_PPV_ARGS(&des))) || !des)return;
    const D3D12_VERSIONED_ROOT_SIGNATURE_DESC* v=nullptr;
    if(SUCCEEDED(des->GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1,&v)) && v) {
        const auto& d=v->Desc_1_1;
        Log("[pair-probe] root %p: %u params, %u static samplers, flags 0x%X\n",static_cast<void*>(root),d.NumParameters,
            d.NumStaticSamplers,unsigned(d.Flags));
        for(UINT i=0;i<d.NumParameters;++i) {
            const auto& p=d.pParameters[i];
            char line[600];int u=snprintf(line,sizeof(line),"[pair-probe]   [%2u] type %u vis %u:",i,unsigned(p.ParameterType),
                                           unsigned(p.ShaderVisibility));
            if(p.ParameterType==D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE)
                for(UINT k=0;k<p.DescriptorTable.NumDescriptorRanges && u<540;++k) {
                    const auto& r=p.DescriptorTable.pDescriptorRanges[k];
                    const char* t=r.RangeType==D3D12_DESCRIPTOR_RANGE_TYPE_CBV?"b":r.RangeType==D3D12_DESCRIPTOR_RANGE_TYPE_SRV?"t":
                                  r.RangeType==D3D12_DESCRIPTOR_RANGE_TYPE_UAV?"u":"s";
                    u+=snprintf(line+u,sizeof(line)-u," %s%u+%d/sp%u@%d f%X",t,r.BaseShaderRegister,int(r.NumDescriptors),
                                r.RegisterSpace,int(r.OffsetInDescriptorsFromTableStart),unsigned(r.Flags));
                }
            else if(p.ParameterType==D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS)
                snprintf(line+u,sizeof(line)-u," b%u/sp%u x%u",p.Constants.ShaderRegister,p.Constants.RegisterSpace,
                         p.Constants.Num32BitValues);
            else snprintf(line+u,sizeof(line)-u," r%u/sp%u",p.Descriptor.ShaderRegister,p.Descriptor.RegisterSpace);
            Log("%s\n",line);
        }
    }
    des->Release();
}
}

bool Wanted() {
    if(!checked) {
        checked=true;
        const auto folder=cvr::paths::DirA(cvr::paths::Folder::Log);
        wanted=!folder.empty() && GetFileAttributesA((folder+"cvr_pair_probe.txt").c_str())!=INVALID_FILE_ATTRIBUTES;
        if(wanted)Log("[pair-probe] marker found: root-parameter detours will be installed\n");
    }
    return wanted;
}
void Install(void** vtable) {
    std::lock_guard lock(installMutex);
    const auto n=originalCount.load(std::memory_order_relaxed);
    for(uint32_t i=0;i<n;++i)if(originals[i].vtable==vtable)return;
    if(n>=originals.size())return;
    auto& o=originals[n];
    o.vtable=vtable;
    o.signature=reinterpret_cast<SigFn>(vtable[30]);o.table=reinterpret_cast<RootFn>(vtable[32]);
    o.constant=reinterpret_cast<ConstFn>(vtable[34]);o.constants=reinterpret_cast<ConstsFn>(vtable[36]);
    o.cbv=reinterpret_cast<RootFn>(vtable[38]);o.srv=reinterpret_cast<RootFn>(vtable[40]);
    o.uav=reinterpret_cast<RootFn>(vtable[42]);o.ib=reinterpret_cast<IbFn>(vtable[43]);
    originalCount.store(n+1,std::memory_order_release);
    const std::pair<size_t,void*> slots[]{{30,reinterpret_cast<void*>(&Signature)},{32,reinterpret_cast<void*>(&Table)},
        {34,reinterpret_cast<void*>(&Constant)},{36,reinterpret_cast<void*>(&Constants)},{38,reinterpret_cast<void*>(&Cbv)},
        {40,reinterpret_cast<void*>(&Srv)},{42,reinterpret_cast<void*>(&Uav)},{43,reinterpret_cast<void*>(&IndexBuffer)}};
    for(const auto& [index,detour]:slots)InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+index),detour);
    tracking.store(true,std::memory_order_release);
    Log("[pair-probe] installed on vtable %p\n",static_cast<void*>(vtable));
}
bool Tracking() {return tracking.load(std::memory_order_relaxed);}
void Reset(ID3D12GraphicsCommandList* list) {
    if(!Tracking())return;
    auto& s=StateOf(list);
    s=State{};
}
void Viewports(ID3D12GraphicsCommandList* list,UINT count,const D3D12_VIEWPORT* viewports) {
    if(!Tracking())return;
    auto& s=StateOf(list);
    if(!count || !viewports){s.viewportX=-1;s.viewportWidth=0;return;}
    s.viewportX=int32_t(viewports[0].TopLeftX+.5f);s.viewportWidth=uint32_t(viewports[0].Width+.5f);
}
void VertexBuffers(ID3D12GraphicsCommandList* list,UINT start,UINT count,const D3D12_VERTEX_BUFFER_VIEW* views) {
    if(!Tracking())return;
    auto& s=StateOf(list);
    s.vb=Fnv(&start,sizeof(start),Fnv(&count,sizeof(count)));
    if(views && count)s.vb=Fnv(views,sizeof(*views)*count,s.vb);
}
void Draw(ID3D12GraphicsCommandList* list,uint32_t node,ID3D12PipelineState* pso,uint32_t kind,const uint32_t (&args)[5]) {
    if(!Tracking())return;
    auto& s=StateOf(list);
    const uint32_t sequence=s.sequence++;
    if(CyberpunkVR_PairProbeState.load(std::memory_order_relaxed)!=2)return;
    const auto wantedNode=CyberpunkVR_PairProbeNode.load(std::memory_order_relaxed);
    if(wantedNode && node!=wantedNode)return;
    const auto serial=CyberpunkVR_PresentTiming[0].load(std::memory_order_relaxed);
    if(serial<=windowStart || serial>windowEnd)return;
    const auto index=CyberpunkVR_PairProbeCount.fetch_add(1,std::memory_order_relaxed);
    if(index>=kCapacity)return;
    auto& r=CyberpunkVR_PairProbeRecords[index];
    r.list=reinterpret_cast<uint64_t>(list);r.pso=reinterpret_cast<uint64_t>(pso);r.root=reinterpret_cast<uint64_t>(s.root);
    r.ib=s.ib;r.vb=s.vb;std::memcpy(r.params,s.params,sizeof(r.params));std::memcpy(r.types,s.types,sizeof(r.types));
    DescribeRoot(s.root);
    r.node=node;r.serial=uint32_t(serial);r.kind=kind;r.viewportX=s.viewportX;r.viewportWidth=s.viewportWidth;
    std::memcpy(r.args,args,sizeof(r.args));r.sequence=sequence;
}
uint32_t BeforeDraw(ID3D12GraphicsCommandList* list,uint32_t node,ID3D12PipelineState* pso,uint32_t kind,
                    const uint32_t (&args)[5],uint32_t startInstance) {
    const uint64_t mask=uint64_t(CyberpunkVR_PairSubstituteMask.load(std::memory_order_relaxed)) |
                        uint64_t(CyberpunkVR_PairSubstituteMaskHigh.load(std::memory_order_relaxed))<<32;
    const bool instance=CyberpunkVR_PairSubstituteInstance.load(std::memory_order_relaxed)!=0;
    if((!mask && !instance) || !Tracking() || kind>2)return startInstance;
    const auto wantedNode=CyberpunkVR_PairProbeNode.load(std::memory_order_relaxed);
    if(wantedNode && node!=wantedNode)return startInstance;
    auto& s=StateOf(list);
    const int eye=s.viewportX==0 ? 0 : (s.viewportX>0 && uint32_t(s.viewportX)==s.viewportWidth ? 1 : -1);
    if(eye==0) {
        if(s.lastEye!=0){s.left.clear();s.rightIndex=0;}
        Partner p{reinterpret_cast<uint64_t>(pso),s.ib,s.vb,kind,startInstance,{},s.tableMask};
        std::memcpy(p.tables,s.tables,sizeof(p.tables));
        s.left.push_back(p);
        s.lastEye=0;
        CyberpunkVR_PairSubstituteCounts[2].fetch_add(1,std::memory_order_relaxed);
        return startInstance;
    }
    if(eye!=1){s.lastEye=-1;return startInstance;}
    if(s.lastEye!=1)s.rightIndex=0;
    s.lastEye=1;
    const uint32_t k=s.rightIndex++;
    const auto* o=OriginalsOf(list);
    if(k>=s.left.size() || !o || !o->table){CyberpunkVR_PairSubstituteCounts[1].fetch_add(1,std::memory_order_relaxed);return startInstance;}
    const Partner& p=s.left[k];
    if(p.pso!=reinterpret_cast<uint64_t>(pso) || p.ib!=s.ib || p.vb!=s.vb || p.kind!=kind) {
        CyberpunkVR_PairSubstituteCounts[1].fetch_add(1,std::memory_order_relaxed);
        return startInstance;
    }
    const uint64_t substitute=mask & p.tableMask & s.tableMask;
    for(uint32_t i=0;i<kParams;++i) {
        if(!(substitute>>i & 1) || p.tables[i]==s.tables[i])continue;
        o->table(list,i,p.tables[i]);
        s.restoreMask|=1ull<<i;
        CyberpunkVR_PairSubstituteCounts[3].fetch_add(1,std::memory_order_relaxed);
    }
    CyberpunkVR_PairSubstituteCounts[0].fetch_add(1,std::memory_order_relaxed);
    return instance ? p.startInstance : startInstance;
}
void AfterDraw(ID3D12GraphicsCommandList* list) {
    if(!Tracking())return;
    auto& s=StateOf(list);
    if(!s.restoreMask)return;
    const auto* o=OriginalsOf(list);
    for(uint32_t i=0;i<kParams;++i)if(s.restoreMask>>i & 1)if(o && o->table)o->table(list,i,s.tables[i]);
    s.restoreMask=0;
}
void Poll() {
    const auto state=CyberpunkVR_PairProbeState.load(std::memory_order_relaxed);
    if(state==2) {
        if(CyberpunkVR_PresentTiming[0].load(std::memory_order_relaxed)>windowEnd+1)CyberpunkVR_PairProbeState.store(3);
        return;
    }
    const auto frames=CyberpunkVR_PairProbeRequest.load(std::memory_order_relaxed);
    if(!frames)return;
    CyberpunkVR_PairProbeRequest.store(0);
    if(!Tracking()){CyberpunkVR_PairProbeState.store(4);return;}
    CyberpunkVR_PairProbeCount.store(0);
    windowStart=CyberpunkVR_PresentTiming[0].load(std::memory_order_relaxed);
    windowEnd=windowStart+std::min<uint32_t>(frames,8);
    CyberpunkVR_PairProbeState.store(2);
}
}
