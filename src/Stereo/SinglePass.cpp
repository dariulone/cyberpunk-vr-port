// SinglePass -- see include/Stereo/SinglePass.hpp.
#include "Stereo/SinglePass.hpp"
#include "Utils/DebugGate.hpp"
#include "Render/SinglePassPipelines.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Render/RenderProbeScope.hpp"
#include <windows.h>
#include <wrl/client.h>
#include <algorithm>
#include <array>
#include <cmath>
#include <cstring>
#include <memory>
#include <mutex>
#include <unordered_map>
#include <vector>

extern void Log(const char*,...);

extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SinglePass{1};   // on (measured: docs/vr-render-optimization-20261104.md)
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SinglePassNode{0x33DDA4};   // RenderElements; 0 = every node
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_SinglePassCounts[10]{};
// 1: while left draws are kept, the engine's state calls are not sent on: the next draw (or command that reads them)
// gets the difference to what the list has. The kept left batch's own state calls then never reach the driver.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SinglePassLazy{1};
// What sent kept left draws before their partner: [slot] a command-list method (its vtable slot), [88] Flush(),
// [89] no originals, [90] a draw outside the node, [91] a draw at another viewport, [92] a left draw whose twin is
// not ready, [93] a right draw without a kept partner, [94] a right draw whose partner differs, [95] a new left batch.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_SinglePassFlushBy[96]{};
// Why a right draw's kept partner did not match: [0] pipeline, [1] kind/arguments, [2] root/tables set/topology,
// [3] index/vertex buffers, [4] stencil/blend/VRS, [5] viewport, [6] scissor, [7] camera tables unset.
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_SinglePassMismatch[12]{};   // [8] blend factor, [9] VRS rate, [10] VRS image
// PROTOTYPE of GPU occlusion culling for the G-buffer's view-instanced draws (docs/vr-render-optimization-20261104.md,
// "GPU occlusion culling prototype"); off by default. 2: a binary occlusion query around every instanced draw, nothing
// culled (the queries' own cost). 1: the same, and a draw found empty in both eyes (one query covers both views of a
// view-instanced draw: conservative across the eyes) the last frames it was drawn is predicated away
// (SetPredication EQUAL_ZERO on its last result). Every CyberpunkVR_OcclusionCullRetest-th frame nothing is
// predicated, so culled draws are tested again: a draw that comes into view can be late by up to that many frames
// (pop-in). A draw is known by its pipeline, buffers, arguments and its occurrence in the frame.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_OcclusionCull{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_OcclusionCullRetest{8};
// [0] queried draws, [1] predicated draws, [2] slots, [3] resolve runs, [4] lists resolved, [5] failures,
// [6] lists reset with unresolved queries
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_OcclusionCullCounts[8]{};
extern std::atomic<uint64_t> CyberpunkVR_PresentTiming[8];
}

namespace cvr::single_pass {
namespace {
auto& Count(size_t i){return CyberpunkVR_SinglePassCounts[i];}
void Cause(size_t i){CyberpunkVR_SinglePassFlushBy[i].fetch_add(1,std::memory_order_relaxed);}
constexpr uint32_t kTables=36,kAll=38,kVb=16;

struct Input {                          // what a draw reads besides its viewports
    ID3D12RootSignature* root{};
    uint64_t tables[kAll]{};
    uint64_t mask{};                    // bit i: table i holds tables[i]
    ID3D12PipelineState* pso{};         // the bound pipeline (for the engine's view: what its SetPipelineState bound)
    D3D12_INDEX_BUFFER_VIEW ib{};
    D3D12_VERTEX_BUFFER_VIEW vb[kVb]{};
    uint32_t vbUsed{};                  // slots below this were set
    D3D12_PRIMITIVE_TOPOLOGY topo{};
    // per-draw output state the engine sets between draws (stencil reference per material, its VRS rate)
    bool stencilKnown{},blendKnown{},rateKnown{},imageKnown{};
    UINT stencil{};
    FLOAT blend[4]{};
    D3D12_SHADING_RATE rate{};D3D12_SHADING_RATE_COMBINER combiners[2]{};
    ID3D12Resource* image{};
};
struct Gfx:Input {
    UINT vpCount{};D3D12_VIEWPORT vp[D3D12_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE]{};
    UINT scCount{};D3D12_RECT sc[D3D12_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE]{};
};
struct Pending {
    ID3D12PipelineState* engine{},*twin{};
    Input in;                           // the engine's state at the left draw
    D3D12_VIEWPORT vp{};D3D12_RECT sc{};
    uint32_t kind{},args[5]{};
    bool sent{};
};
struct List {
    Gfx eng,act;                        // what the engine set, what the list has
    bool dirty{};                       // act may differ from eng
    ID3D12PipelineState* engPso{};      // the engine's own pipeline
    Pipeline* rec{};ID3D12PipelineState* recPso{};
    std::vector<Pending> pending;
    size_t cursor{},unsent{};
    int phase{};                        // 1 left batch, 2 right batch
    uint32_t node{};                    // the render-graph node of the batch
    bool broken{};
    // the bound render targets (an eye's batch binds the same ones again: nothing the kept draws need changes)
    bool targetsKnown{};UINT targetCount{};BOOL targetsContiguous{};SIZE_T targets[8]{},depthTarget{};
};
std::mutex listMutex;
std::unordered_map<ID3D12GraphicsCommandList*,std::unique_ptr<List>> lists;
thread_local ID3D12GraphicsCommandList* t_list{};
thread_local List* t_state{};
List& Of(ID3D12GraphicsCommandList* list) {
    if(t_list==list && t_state)return *t_state;
    std::lock_guard lock(listMutex);
    auto& slot=lists[list];
    if(!slot)slot=std::make_unique<List>();
    t_list=list;t_state=slot.get();
    return *slot;
}
bool Hold(const List& l) {
    return l.unsent && CyberpunkVR_SinglePassLazy.load(std::memory_order_relaxed) && CyberpunkVR_SinglePass.load(std::memory_order_relaxed);
}
using Hook=cvr::detail::CommandListVtableHook;
const Hook* Originals(ID3D12GraphicsCommandList* list){return cvr::detail::command_list_hook_entry(list);}

// ---- applying state ------------------------------------------------------------------------------------------------
void ApplyRoot(ID3D12GraphicsCommandList* list,const Hook* e,List& l,ID3D12RootSignature* root,const uint64_t* tables,uint64_t mask,
               uint32_t count) {
    if(l.act.root!=root){e->groot_original(list,root);l.act.root=root;l.act.mask=0;}
    for(uint32_t i=0;i<count;++i) {
        if(!(mask>>i & 1))continue;
        if((l.act.mask>>i & 1) && l.act.tables[i]==tables[i])continue;
        e->grootdt_original(list,i,D3D12_GPU_DESCRIPTOR_HANDLE{tables[i]});
        l.act.tables[i]=tables[i];l.act.mask|=1ull<<i;
    }
}
bool SameViews(const D3D12_VERTEX_BUFFER_VIEW* a,const D3D12_VERTEX_BUFFER_VIEW* b,uint32_t n){return !std::memcmp(a,b,sizeof(*a)*n);}
void ApplyInput(ID3D12GraphicsCommandList* list,const Hook* e,List& l,const Input& t) {
    if(t.pso && l.act.pso!=t.pso){e->setpso_original(list,t.pso);l.act.pso=t.pso;}
    if(std::memcmp(&l.act.ib,&t.ib,sizeof(t.ib))){if(e->iaib_original)e->iaib_original(list,&t.ib);l.act.ib=t.ib;}
    const uint32_t n=std::max(l.act.vbUsed,t.vbUsed);
    if(n && !SameViews(l.act.vb,t.vb,n)){e->iavb_original(list,0,n,t.vb);std::memcpy(l.act.vb,t.vb,sizeof(t.vb[0])*n);l.act.vbUsed=n;}
    if(l.act.topo!=t.topo && t.topo){if(e->topology_original)e->topology_original(list,t.topo);l.act.topo=t.topo;}
    if(t.stencilKnown && (!l.act.stencilKnown || l.act.stencil!=t.stencil) && e->stencil_original) {
        e->stencil_original(list,t.stencil);l.act.stencil=t.stencil;l.act.stencilKnown=true;
    }
    if(t.blendKnown && (!l.act.blendKnown || std::memcmp(l.act.blend,t.blend,sizeof(t.blend))) && e->blend_factor_original) {
        e->blend_factor_original(list,t.blend);std::memcpy(l.act.blend,t.blend,sizeof(t.blend));l.act.blendKnown=true;
    }
    auto* list5=static_cast<ID3D12GraphicsCommandList5*>(list);
    if(t.rateKnown && (!l.act.rateKnown || l.act.rate!=t.rate || std::memcmp(l.act.combiners,t.combiners,sizeof(t.combiners))) &&
       e->shading_rate_original) {
        e->shading_rate_original(list5,t.rate,t.combiners);l.act.rate=t.rate;std::memcpy(l.act.combiners,t.combiners,sizeof(t.combiners));
        l.act.rateKnown=true;
    }
    if(t.imageKnown && (!l.act.imageKnown || l.act.image!=t.image) && e->shading_image_original) {
        e->shading_image_original(list5,t.image);l.act.image=t.image;l.act.imageKnown=true;
    }
}
void ApplyViews(ID3D12GraphicsCommandList* list,const Hook* e,List& l,UINT vpCount,const D3D12_VIEWPORT* vp,UINT scCount,const D3D12_RECT* sc) {
    if(vpCount && (l.act.vpCount!=vpCount || std::memcmp(l.act.vp,vp,sizeof(*vp)*vpCount))) {
        e->viewports_original(list,vpCount,vp);l.act.vpCount=vpCount;std::memcpy(l.act.vp,vp,sizeof(*vp)*vpCount);
    }
    if(scCount && (l.act.scCount!=scCount || std::memcmp(l.act.sc,sc,sizeof(*sc)*scCount))) {
        e->scissor_original(list,scCount,sc);l.act.scCount=scCount;std::memcpy(l.act.sc,sc,sizeof(*sc)*scCount);
    }
}
// The engine's state back on the list.
void Restore(ID3D12GraphicsCommandList* list,List& l) {
    if(!l.dirty)return;
    const Hook* e=Originals(list);if(!e)return;
    if(l.eng.root)ApplyRoot(list,e,l,l.eng.root,l.eng.tables,l.eng.mask,kTables);
    ApplyInput(list,e,l,l.eng);
    ApplyViews(list,e,l,l.eng.vpCount,l.eng.vp,l.eng.scCount,l.eng.sc);
    l.dirty=false;
}
void Send(ID3D12GraphicsCommandList* list,const Hook* e,uint32_t kind,const uint32_t (&a)[5]) {
    if(kind==2)e->drawidx_original(list,a[0],a[1],a[2],INT(a[3]),a[4]);
    else e->draw_original(list,a[0],a[1],a[2],a[3]);
}
// A kept left draw, as the engine would have sent it.
void Emit(ID3D12GraphicsCommandList* list,const Hook* e,List& l,Pending& p) {
    l.dirty=true;
    ApplyRoot(list,e,l,p.in.root,p.in.tables,p.in.mask,kTables);
    ApplyInput(list,e,l,p.in);
    ApplyViews(list,e,l,1,&p.vp,1,&p.sc);
    Send(list,e,p.kind,p.args);
    p.sent=true;--l.unsent;
    Count(2)++;
}
void FlushList(ID3D12GraphicsCommandList* list,List& l) {
    if(l.unsent) {
        const Hook* e=Originals(list);
        if(e){for(auto& p:l.pending)if(!p.sent)Emit(list,e,l,p);Count(5)++;}
        l.unsent=0;
    }
    Restore(list,l);
}
namespace occ {void Draw(ID3D12GraphicsCommandList*,const Hook*,const List&,uint32_t,const uint32_t (&)[5]);}
// Both eyes in one draw: the right eye's state as the engine bound it, the left eye's camera tables in 36/37.
void Instanced(ID3D12GraphicsCommandList* list,const Hook* e,List& l,Pending& p,uint32_t kind,const uint32_t (&args)[5]) {
    ID3D12RootSignature* twinRoot=TwinRoot(l.eng.root);
    l.dirty=true;
    uint64_t tables[kAll];std::memcpy(tables,l.eng.tables,sizeof(uint64_t)*kTables);
    tables[36]=p.in.tables[0];tables[37]=p.in.tables[18];
    if(l.act.root!=twinRoot)Count(9)++;
    ApplyRoot(list,e,l,twinRoot,tables,l.eng.mask|(3ull<<36),kAll);
    Input input=l.eng;input.pso=p.twin;
    ApplyInput(list,e,l,input);
    const D3D12_VIEWPORT vp[2]{p.vp,l.eng.vp[0]};
    const D3D12_RECT sc[2]{p.sc,l.eng.sc[0]};
    ApplyViews(list,e,l,2,vp,2,sc);
    occ::Draw(list,e,l,kind,args);
    p.sent=true;--l.unsent;
}
bool Near(float a,float b){return std::fabs(a-b)<0.5f;}
bool Mismatch(size_t why){CyberpunkVR_SinglePassMismatch[why].fetch_add(1,std::memory_order_relaxed);return false;}
bool Matches(const Pending& p,const List& l,uint32_t kind,const uint32_t (&args)[5]) {
    const Input& a=p.in;const Gfx& b=l.eng;
    if(p.engine!=l.engPso || a.pso!=b.pso)return Mismatch(0);
    if(p.kind!=kind)return Mismatch(1);
    if(a.root!=b.root || a.mask!=b.mask || a.topo!=b.topo)return Mismatch(2);
    const uint32_t instanceArg=kind==2?4:3;
    for(uint32_t i=0;i<5;++i)if(i!=instanceArg && p.args[i]!=args[i])return Mismatch(1);
    // the index buffer of an indexed draw and the vertex buffers its input layout reads (other slots may hold
    // anything the batch bound before)
    if(kind==2 && std::memcmp(&a.ib,&b.ib,sizeof(a.ib)))return Mismatch(3);
    const uint32_t slots=InputSlots(l.rec);
    for(uint32_t i=0;i<kVb;++i) {
        if(!(slots>>i & 1))continue;
        const D3D12_VERTEX_BUFFER_VIEW none{};
        const auto& x=i<a.vbUsed?a.vb[i]:none;const auto& y=i<b.vbUsed?b.vb[i]:none;
        if(std::memcmp(&x,&y,sizeof(x)))return Mismatch(3);
    }
    if(a.stencilKnown!=b.stencilKnown || a.stencil!=b.stencil)return Mismatch(4);
    if(a.blendKnown!=b.blendKnown || std::memcmp(a.blend,b.blend,sizeof(a.blend)))return Mismatch(8);
    if(a.rateKnown!=b.rateKnown || a.rate!=b.rate || std::memcmp(a.combiners,b.combiners,sizeof(a.combiners)))return Mismatch(9);
    if(a.imageKnown!=b.imageKnown || a.image!=b.image)return Mismatch(10);
    const auto& u=p.vp;const auto& v=b.vp[0];
    if(!Near(u.Width,v.Width) || !Near(u.Height,v.Height) || !Near(u.TopLeftY,v.TopLeftY) || u.MinDepth!=v.MinDepth ||
       u.MaxDepth!=v.MaxDepth || !Near(v.TopLeftX,u.TopLeftX+u.Width))return Mismatch(5);
    const auto& c=p.sc;const auto& d=b.sc[0];
    if(b.scCount!=1 || c.top!=d.top || c.bottom!=d.bottom || c.right-c.left!=d.right-d.left)return Mismatch(6);
    return (a.mask & 1) && (a.mask>>18 & 1)?true:Mismatch(7);
}
int EyeOf(const List& l) {
    if(l.eng.vpCount!=1 || l.eng.scCount!=1)return -1;
    const auto& v=l.eng.vp[0];
    if(v.Width<=0)return -1;
    if(Near(v.TopLeftX,0))return 0;
    if(Near(v.TopLeftX,v.Width))return 1;
    return -1;
}

// ---- detours: chained over whatever the vtable had --------------------------------------------------------------
constexpr size_t kSlots=81;
struct Chain {void** vtable{};void* next[kSlots]{};};
std::array<Chain,16> chains;
std::atomic<uint32_t> chainCount{0};
std::mutex chainMutex;
const Chain* ChainOf(void* self) {
    void** vtable=*reinterpret_cast<void***>(self);
    const auto n=chainCount.load(std::memory_order_acquire);
    for(uint32_t i=0;i<n;++i)if(chains[i].vtable==vtable)return &chains[i];
    return nullptr;
}

// ---- occlusion culling prototype (CyberpunkVR_OcclusionCull) -------------------------------------------------------
namespace occ {
using Microsoft::WRL::ComPtr;
using L=ID3D12GraphicsCommandList;
constexpr uint32_t kCapacity=1u<<17;          // query indices: two halves, even and odd frames
constexpr uint32_t kHalf=kCapacity/2,kChunk=256;  // a list takes its indices in runs of 256: one resolve per run
constexpr uint32_t kDraws=1u<<16;                 // draws known (slots)
auto& Stat(size_t i){return CyberpunkVR_OcclusionCullCounts[i];}
struct Key {uint64_t pso{},ib{},vb{};uint32_t args[4]{},kind{},k{};
            bool operator==(const Key& o) const {return !std::memcmp(this,&o,sizeof(Key));}};
struct KeyHash {size_t operator()(const Key& k) const {
    uint64_t h=1469598103934665603ull;const auto* b=reinterpret_cast<const unsigned char*>(&k);
    for(size_t i=0;i<sizeof(k);++i){h^=b[i];h*=1099511628211ull;}return size_t(h);}};
std::mutex mutex;
ComPtr<ID3D12QueryHeap> heap;ComPtr<ID3D12Resource> results;bool failed{};
std::unordered_map<Key,uint32_t,KeyHash> slots;std::unordered_map<Key,uint32_t,KeyHash> seen;
uint64_t frame=~0ull;
std::vector<uint64_t> lastUse;std::vector<uint8_t> streak;std::vector<uint32_t> lastIndex;
uint32_t frameNext{};                              // next free index of this frame's half
struct Chunk {uint64_t frame=~0ull;uint32_t next{},end{};};
std::unordered_map<L*,Chunk> chunks;
std::unordered_map<L*,std::vector<uint32_t>> used;  // query indices this list ended (resolved at its Close)
// The query heap and the buffer the results are resolved into and predicated from (kept in PREDICATION between
// resolves). Made once, never released.
bool Ready(L* list) {
    if(heap)return true;
    if(failed)return false;
    ComPtr<ID3D12Device> device;
    D3D12_QUERY_HEAP_DESC query{};query.Type=D3D12_QUERY_HEAP_TYPE_OCCLUSION;query.Count=kCapacity;
    D3D12_HEAP_PROPERTIES properties{};properties.Type=D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=UINT64(kCapacity)*8;desc.Height=1;
    desc.DepthOrArraySize=1;desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    if(FAILED(list->GetDevice(IID_PPV_ARGS(&device))) || FAILED(device->CreateQueryHeap(&query,IID_PPV_ARGS(&heap))) ||
       FAILED(device->CreateCommittedResource(&properties,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_PREDICATION,nullptr,
                                              IID_PPV_ARGS(&results)))) {
        heap.Reset();results.Reset();failed=true;Stat(5)++;
        Log("[occlusion] query heap / predication buffer not created: prototype off\n");
        return false;
    }
    lastUse.assign(kDraws,~0ull);streak.assign(kDraws,0);lastIndex.assign(kDraws,0);
    Log("[occlusion] prototype on: %u query slots\n",kCapacity);
    return true;
}
using QueryFn=void(STDMETHODCALLTYPE*)(L*,ID3D12QueryHeap*,D3D12_QUERY_TYPE,UINT);
using PredicateFn=void(STDMETHODCALLTYPE*)(L*,ID3D12Resource*,UINT64,D3D12_PREDICATION_OP);
using ResolveFn=void(STDMETHODCALLTYPE*)(L*,ID3D12QueryHeap*,D3D12_QUERY_TYPE,UINT,UINT,ID3D12Resource*,UINT64);
using BarrierFn=void(STDMETHODCALLTYPE*)(L*,UINT,const D3D12_RESOURCE_BARRIER*);
void Draw(L* list,const Hook* e,const List& l,uint32_t kind,const uint32_t (&a)[5]) {
    const uint32_t mode=CyberpunkVR_OcclusionCull.load(std::memory_order_relaxed);
    const Chain* chain=mode?ChainOf(list):nullptr;
    if(!chain || !chain->next[52] || !chain->next[53] || !chain->next[55]){Send(list,e,kind,a);return;}
    uint32_t query{},previous{};bool predicate{};
    {
        std::lock_guard lock(mutex);
        if(!Ready(list)){Send(list,e,kind,a);return;}
        const uint64_t now=CyberpunkVR_PresentTiming[0].load(std::memory_order_relaxed);
        if(now!=frame){frame=now;seen.clear();frameNext=0;}
        // this draw's query index: from the list's run in this frame's half
        auto& chunk=chunks[list];
        if(chunk.frame!=now || chunk.next==chunk.end) {
            if(frameNext+kChunk>kHalf){Send(list,e,kind,a);return;}
            chunk.frame=now;chunk.next=frameNext;chunk.end=frameNext+kChunk;frameNext+=kChunk;
        }
        query=uint32_t(now&1)*kHalf+chunk.next++;
        Key key{};key.pso=reinterpret_cast<uint64_t>(l.act.pso);key.ib=kind==2?l.act.ib.BufferLocation:0;
        key.vb=l.act.vbUsed?l.act.vb[0].BufferLocation:0;key.kind=kind;
        for(uint32_t i=0;i<3;++i)key.args[i]=a[i];
        key.args[3]=kind==2?a[3]:0;   // not the instance offset: the engine's per-frame instance data moves
        key.k=seen[key]++;
        auto it=slots.find(key);
        if(it==slots.end()) {
            if(slots.size()>=kDraws){--chunk.next;Send(list,e,kind,a);return;}
            it=slots.emplace(key,uint32_t(slots.size())).first;Stat(2)++;
        }
        const uint32_t slot=it->second;
        const bool lastFrame=lastUse[slot]+1==now;
        if(lastUse[slot]!=now){streak[slot]=lastFrame?uint8_t(std::min(streak[slot]+1,255)):uint8_t(1);lastUse[slot]=now;}
        const uint32_t retest=std::max(1u,CyberpunkVR_OcclusionCullRetest.load(std::memory_order_relaxed));
        // drawn the frames before this one (its last result is resolved), and not a frame that tests everything again
        predicate=mode==1 && lastFrame && streak[slot]>=3 && now%retest!=0;
        previous=lastIndex[slot];lastIndex[slot]=query;
        used[list].push_back(query);
    }
    cvr::stereo::probe::Scope internal;
    if(predicate){reinterpret_cast<PredicateFn>(chain->next[55])(list,results.Get(),UINT64(previous)*8,D3D12_PREDICATION_OP_EQUAL_ZERO);Stat(1)++;}
    reinterpret_cast<QueryFn>(chain->next[52])(list,heap.Get(),D3D12_QUERY_TYPE_BINARY_OCCLUSION,query);
    Send(list,e,kind,a);
    reinterpret_cast<QueryFn>(chain->next[53])(list,heap.Get(),D3D12_QUERY_TYPE_BINARY_OCCLUSION,query);
    if(predicate)reinterpret_cast<PredicateFn>(chain->next[55])(list,nullptr,0,D3D12_PREDICATION_OP_EQUAL_ZERO);
    Stat(0)++;
}
// At Close: this list's queries resolved into the predication buffer (outside any render pass), in runs of
// consecutive slots; the next frame's draws predicate on them.
void Close(L* list) {
    std::vector<uint32_t> mine;
    {
        std::lock_guard lock(mutex);
        const auto it=used.find(list);if(it==used.end())return;
        mine.swap(it->second);
    }
    const Chain* chain=ChainOf(list);
    if(mine.empty() || !results || !chain || !chain->next[26] || !chain->next[54])return;
    std::sort(mine.begin(),mine.end());mine.erase(std::unique(mine.begin(),mine.end()),mine.end());
    cvr::stereo::probe::Scope internal;
    D3D12_RESOURCE_BARRIER barrier{};barrier.Type=D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;barrier.Transition.pResource=results.Get();
    barrier.Transition.Subresource=D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barrier.Transition.StateBefore=D3D12_RESOURCE_STATE_PREDICATION;barrier.Transition.StateAfter=D3D12_RESOURCE_STATE_COPY_DEST;
    reinterpret_cast<BarrierFn>(chain->next[26])(list,1,&barrier);
    for(size_t i=0;i<mine.size();) {
        size_t j=i+1;while(j<mine.size() && mine[j]==mine[j-1]+1)++j;
        reinterpret_cast<ResolveFn>(chain->next[54])(list,heap.Get(),D3D12_QUERY_TYPE_BINARY_OCCLUSION,mine[i],UINT(j-i),
                                                     results.Get(),UINT64(mine[i])*8);
        Stat(3)++;i=j;
    }
    std::swap(barrier.Transition.StateBefore,barrier.Transition.StateAfter);
    reinterpret_cast<BarrierFn>(chain->next[26])(list,1,&barrier);
    Stat(4)++;
}
void Reset(L* list) {
    std::lock_guard lock(mutex);
    const auto it=used.find(list);
    if(it!=used.end() && !it->second.empty()){Stat(6)++;it->second.clear();}
    chunks.erase(list);
}
}
std::atomic<bool> active{false};   // a material root exists: state is kept
template<size_t Slot,class Sig> struct Flushing;
template<size_t Slot,class R,class Self,class... A> struct Flushing<Slot,R(STDMETHODCALLTYPE*)(Self*,A...)> {
    static R STDMETHODCALLTYPE Detour(Self* self,A... a) {
        auto* list=reinterpret_cast<ID3D12GraphicsCommandList*>(self);
        if(active.load(std::memory_order_relaxed)){auto& l=Of(list);if(l.unsent)Cause(Slot);if(l.unsent || l.dirty)FlushList(list,l);}
        if constexpr(Slot==9)occ::Close(list);   // Close: the occlusion prototype's queries resolved
        using Fn=R(STDMETHODCALLTYPE*)(Self*,A...);
        return reinterpret_cast<Fn>(ChainOf(self)->next[Slot])(self,a...);
    }
};
using L=ID3D12GraphicsCommandList;using L1=ID3D12GraphicsCommandList1;using L2=ID3D12GraphicsCommandList2;
using L4=ID3D12GraphicsCommandList4;using L5=ID3D12GraphicsCommandList5;using L6=ID3D12GraphicsCommandList6;
using L7=ID3D12GraphicsCommandList7;
#define CVR_SP(slot,...) {slot,reinterpret_cast<void*>(&Flushing<slot,__VA_ARGS__>::Detour)}
struct FlushSlot {size_t slot;void* detour;};
const FlushSlot kFlushSlots[]{
    CVR_SP(9,HRESULT(STDMETHODCALLTYPE*)(L*)),
    CVR_SP(14,void(STDMETHODCALLTYPE*)(L*,UINT,UINT,UINT)),
    CVR_SP(15,void(STDMETHODCALLTYPE*)(L*,ID3D12Resource*,UINT64,ID3D12Resource*,UINT64,UINT64)),
    CVR_SP(16,void(STDMETHODCALLTYPE*)(L*,const D3D12_TEXTURE_COPY_LOCATION*,UINT,UINT,UINT,const D3D12_TEXTURE_COPY_LOCATION*,const D3D12_BOX*)),
    CVR_SP(17,void(STDMETHODCALLTYPE*)(L*,ID3D12Resource*,ID3D12Resource*)),
    CVR_SP(18,void(STDMETHODCALLTYPE*)(L*,ID3D12Resource*,const D3D12_TILED_RESOURCE_COORDINATE*,const D3D12_TILE_REGION_SIZE*,ID3D12Resource*,UINT64,D3D12_TILE_COPY_FLAGS)),
    CVR_SP(19,void(STDMETHODCALLTYPE*)(L*,ID3D12Resource*,UINT,ID3D12Resource*,UINT,DXGI_FORMAT)),
    CVR_SP(26,void(STDMETHODCALLTYPE*)(L*,UINT,const D3D12_RESOURCE_BARRIER*)),
    CVR_SP(27,void(STDMETHODCALLTYPE*)(L*,ID3D12GraphicsCommandList*)),
    CVR_SP(28,void(STDMETHODCALLTYPE*)(L*,UINT,ID3D12DescriptorHeap* const*)),
    CVR_SP(34,void(STDMETHODCALLTYPE*)(L*,UINT,UINT,UINT)),
    CVR_SP(36,void(STDMETHODCALLTYPE*)(L*,UINT,UINT,const void*,UINT)),
    CVR_SP(38,void(STDMETHODCALLTYPE*)(L*,UINT,D3D12_GPU_VIRTUAL_ADDRESS)),
    CVR_SP(40,void(STDMETHODCALLTYPE*)(L*,UINT,D3D12_GPU_VIRTUAL_ADDRESS)),
    CVR_SP(42,void(STDMETHODCALLTYPE*)(L*,UINT,D3D12_GPU_VIRTUAL_ADDRESS)),
    CVR_SP(45,void(STDMETHODCALLTYPE*)(L*,UINT,UINT,const D3D12_STREAM_OUTPUT_BUFFER_VIEW*)),
    CVR_SP(47,void(STDMETHODCALLTYPE*)(L*,D3D12_CPU_DESCRIPTOR_HANDLE,D3D12_CLEAR_FLAGS,FLOAT,UINT8,UINT,const D3D12_RECT*)),
    CVR_SP(48,void(STDMETHODCALLTYPE*)(L*,D3D12_CPU_DESCRIPTOR_HANDLE,const FLOAT*,UINT,const D3D12_RECT*)),
    CVR_SP(49,void(STDMETHODCALLTYPE*)(L*,D3D12_GPU_DESCRIPTOR_HANDLE,D3D12_CPU_DESCRIPTOR_HANDLE,ID3D12Resource*,const UINT*,UINT,const D3D12_RECT*)),
    CVR_SP(50,void(STDMETHODCALLTYPE*)(L*,D3D12_GPU_DESCRIPTOR_HANDLE,D3D12_CPU_DESCRIPTOR_HANDLE,ID3D12Resource*,const FLOAT*,UINT,const D3D12_RECT*)),
    CVR_SP(51,void(STDMETHODCALLTYPE*)(L*,ID3D12Resource*,const D3D12_DISCARD_REGION*)),
    CVR_SP(52,void(STDMETHODCALLTYPE*)(L*,ID3D12QueryHeap*,D3D12_QUERY_TYPE,UINT)),
    CVR_SP(53,void(STDMETHODCALLTYPE*)(L*,ID3D12QueryHeap*,D3D12_QUERY_TYPE,UINT)),
    CVR_SP(54,void(STDMETHODCALLTYPE*)(L*,ID3D12QueryHeap*,D3D12_QUERY_TYPE,UINT,UINT,ID3D12Resource*,UINT64)),
    CVR_SP(55,void(STDMETHODCALLTYPE*)(L*,ID3D12Resource*,UINT64,D3D12_PREDICATION_OP)),
    CVR_SP(59,void(STDMETHODCALLTYPE*)(L*,ID3D12CommandSignature*,UINT,ID3D12Resource*,UINT64,ID3D12Resource*,UINT64)),
    CVR_SP(60,void(STDMETHODCALLTYPE*)(L1*,ID3D12Resource*,UINT64,ID3D12Resource*,UINT64,UINT,ID3D12Resource* const*,const D3D12_SUBRESOURCE_RANGE_UINT64*)),
    CVR_SP(61,void(STDMETHODCALLTYPE*)(L1*,ID3D12Resource*,UINT64,ID3D12Resource*,UINT64,UINT,ID3D12Resource* const*,const D3D12_SUBRESOURCE_RANGE_UINT64*)),
    CVR_SP(62,void(STDMETHODCALLTYPE*)(L1*,FLOAT,FLOAT)),
    CVR_SP(63,void(STDMETHODCALLTYPE*)(L1*,UINT,UINT,D3D12_SAMPLE_POSITION*)),
    CVR_SP(64,void(STDMETHODCALLTYPE*)(L1*,ID3D12Resource*,UINT,UINT,UINT,ID3D12Resource*,UINT,D3D12_RECT*,DXGI_FORMAT,D3D12_RESOLVE_MODE)),
    CVR_SP(65,void(STDMETHODCALLTYPE*)(L1*,UINT)),
    CVR_SP(66,void(STDMETHODCALLTYPE*)(L2*,UINT,const D3D12_WRITEBUFFERIMMEDIATE_PARAMETER*,const D3D12_WRITEBUFFERIMMEDIATE_MODE*)),
    CVR_SP(68,void(STDMETHODCALLTYPE*)(L4*,UINT,const D3D12_RENDER_PASS_RENDER_TARGET_DESC*,const D3D12_RENDER_PASS_DEPTH_STENCIL_DESC*,D3D12_RENDER_PASS_FLAGS)),
    CVR_SP(69,void(STDMETHODCALLTYPE*)(L4*)),
    CVR_SP(70,void(STDMETHODCALLTYPE*)(L4*,ID3D12MetaCommand*,const void*,SIZE_T)),
    CVR_SP(71,void(STDMETHODCALLTYPE*)(L4*,ID3D12MetaCommand*,const void*,SIZE_T)),
    CVR_SP(72,void(STDMETHODCALLTYPE*)(L4*,const D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC*,UINT,const D3D12_RAYTRACING_ACCELERATION_STRUCTURE_POSTBUILD_INFO_DESC*)),
    CVR_SP(73,void(STDMETHODCALLTYPE*)(L4*,const D3D12_RAYTRACING_ACCELERATION_STRUCTURE_POSTBUILD_INFO_DESC*,UINT,const D3D12_GPU_VIRTUAL_ADDRESS*)),
    CVR_SP(74,void(STDMETHODCALLTYPE*)(L4*,D3D12_GPU_VIRTUAL_ADDRESS,D3D12_GPU_VIRTUAL_ADDRESS,D3D12_RAYTRACING_ACCELERATION_STRUCTURE_COPY_MODE)),
    CVR_SP(76,void(STDMETHODCALLTYPE*)(L4*,const D3D12_DISPATCH_RAYS_DESC*)),
    CVR_SP(79,void(STDMETHODCALLTYPE*)(L6*,UINT,UINT,UINT)),
    CVR_SP(80,void(STDMETHODCALLTYPE*)(L7*,UINT32,const D3D12_BARRIER_GROUP*)),
};
#undef CVR_SP
// ClearState: a flush, then nothing of the old state holds.
void STDMETHODCALLTYPE ClearState(L* self,ID3D12PipelineState* pso) {
    if(active.load(std::memory_order_relaxed)){auto& l=Of(self);FlushList(self,l);l.eng=l.act=Gfx{};l.engPso=nullptr;l.dirty=false;}
    reinterpret_cast<void(STDMETHODCALLTYPE*)(L*,ID3D12PipelineState*)>(ChainOf(self)->next[11])(self,pso);
}
// SetPipelineState1 (a ray-tracing state object) takes the pipeline slot.
void STDMETHODCALLTYPE StateObject(L4* self,ID3D12StateObject* object) {
    auto* list=reinterpret_cast<L*>(self);
    if(active.load(std::memory_order_relaxed)){auto& l=Of(list);FlushList(list,l);l.eng.pso=l.act.pso=nullptr;l.engPso=nullptr;}
    reinterpret_cast<void(STDMETHODCALLTYPE*)(L4*,ID3D12StateObject*)>(ChainOf(self)->next[75])(self,object);
}
void STDMETHODCALLTYPE Topology(L* self,D3D12_PRIMITIVE_TOPOLOGY t) {
    if(active.load(std::memory_order_relaxed)){auto& l=Of(self);if(Hold(l)){l.eng.topo=t;l.dirty=true;return;}l.eng.topo=l.act.topo=t;}
    reinterpret_cast<void(STDMETHODCALLTYPE*)(L*,D3D12_PRIMITIVE_TOPOLOGY)>(ChainOf(self)->next[20])(self,t);
}
void STDMETHODCALLTYPE Root(L* self,ID3D12RootSignature* root) {
    if(active.load(std::memory_order_relaxed)) {
        auto& l=Of(self);
        if(l.unsent && root!=l.eng.root){Cause(30);FlushList(self,l);}   // the kept draws need the material root
        // Setting the bound root again keeps its arguments; another root starts without any.
        if(root!=l.act.root){l.act.mask=0;if(root==l.eng.root && l.eng.mask)l.dirty=true;}
        if(root!=l.eng.root)l.eng.mask=0;
        l.eng.root=l.act.root=root;
    }
    reinterpret_cast<void(STDMETHODCALLTYPE*)(L*,ID3D12RootSignature*)>(ChainOf(self)->next[30])(self,root);
}
void STDMETHODCALLTYPE Table(L* self,UINT index,D3D12_GPU_DESCRIPTOR_HANDLE handle) {
    if(active.load(std::memory_order_relaxed) && index<kTables) {
        auto& l=Of(self);
        l.eng.tables[index]=handle.ptr;l.eng.mask|=1ull<<index;
        if(Hold(l) && l.act.root==l.eng.root){l.dirty=true;return;}
        l.act.tables[index]=handle.ptr;l.act.mask|=1ull<<index;
    }
    reinterpret_cast<void(STDMETHODCALLTYPE*)(L*,UINT,D3D12_GPU_DESCRIPTOR_HANDLE)>(ChainOf(self)->next[32])(self,index,handle);
}
void STDMETHODCALLTYPE Targets(L* self,UINT count,const D3D12_CPU_DESCRIPTOR_HANDLE* rt,BOOL contiguous,const D3D12_CPU_DESCRIPTOR_HANDLE* ds) {
    if(active.load(std::memory_order_relaxed)) {
        auto& l=Of(self);
        const UINT n=rt?std::min<UINT>(count,8):0;const UINT listed=contiguous?std::min<UINT>(n,1):n;
        bool same=l.targetsKnown && count==l.targetCount && contiguous==l.targetsContiguous && (ds?ds->ptr:0)==l.depthTarget;
        for(UINT i=0;same && i<listed;++i)same=rt[i].ptr==l.targets[i];
        if(!same){if(l.unsent)Cause(46);if(l.unsent || l.dirty)FlushList(self,l);}
        l.targetsKnown=count<=8;l.targetCount=count;l.targetsContiguous=contiguous;l.depthTarget=ds?ds->ptr:0;
        for(UINT i=0;i<listed;++i)l.targets[i]=rt[i].ptr;
    }
    reinterpret_cast<void(STDMETHODCALLTYPE*)(L*,UINT,const D3D12_CPU_DESCRIPTOR_HANDLE*,BOOL,const D3D12_CPU_DESCRIPTOR_HANDLE*)>(
        ChainOf(self)->next[46])(self,count,rt,contiguous,ds);
}
void STDMETHODCALLTYPE StencilRef(L* self,UINT value) {
    if(active.load(std::memory_order_relaxed)) {
        auto& l=Of(self);l.eng.stencil=value;l.eng.stencilKnown=true;
        if(Hold(l)){l.dirty=true;return;}
        l.act.stencil=value;l.act.stencilKnown=true;
    }
    reinterpret_cast<void(STDMETHODCALLTYPE*)(L*,UINT)>(ChainOf(self)->next[24])(self,value);
}
void STDMETHODCALLTYPE BlendFactor(L* self,const FLOAT* factor) {
    reinterpret_cast<void(STDMETHODCALLTYPE*)(L*,const FLOAT*)>(ChainOf(self)->next[23])(self,factor);
    if(active.load(std::memory_order_relaxed)) {
        auto& l=Of(self);static constexpr FLOAT ones[4]{1,1,1,1};const FLOAT* f=factor?factor:ones;
        std::memcpy(l.eng.blend,f,sizeof(l.eng.blend));std::memcpy(l.act.blend,f,sizeof(l.act.blend));l.eng.blendKnown=l.act.blendKnown=true;
    }
}
void STDMETHODCALLTYPE ShadingRate(L5* self,D3D12_SHADING_RATE rate,const D3D12_SHADING_RATE_COMBINER* combiners) {
    reinterpret_cast<void(STDMETHODCALLTYPE*)(L5*,D3D12_SHADING_RATE,const D3D12_SHADING_RATE_COMBINER*)>(ChainOf(self)->next[77])(self,rate,combiners);
    if(active.load(std::memory_order_relaxed)) {
        auto& l=Of(reinterpret_cast<L*>(self));
        const D3D12_SHADING_RATE_COMBINER c[2]{combiners?combiners[0]:D3D12_SHADING_RATE_COMBINER_PASSTHROUGH,
                                               combiners?combiners[1]:D3D12_SHADING_RATE_COMBINER_PASSTHROUGH};
        l.eng.rate=l.act.rate=rate;std::memcpy(l.eng.combiners,c,sizeof(c));std::memcpy(l.act.combiners,c,sizeof(c));
        l.eng.rateKnown=l.act.rateKnown=true;
    }
}
void STDMETHODCALLTYPE ShadingImage(L5* self,ID3D12Resource* image) {
    reinterpret_cast<void(STDMETHODCALLTYPE*)(L5*,ID3D12Resource*)>(ChainOf(self)->next[78])(self,image);
    if(active.load(std::memory_order_relaxed)){auto& l=Of(reinterpret_cast<L*>(self));l.eng.image=l.act.image=image;l.eng.imageKnown=l.act.imageKnown=true;}
}
void STDMETHODCALLTYPE IndexBuffer(L* self,const D3D12_INDEX_BUFFER_VIEW* view) {
    if(active.load(std::memory_order_relaxed)) {
        auto& l=Of(self);l.eng.ib=view?*view:D3D12_INDEX_BUFFER_VIEW{};
        if(Hold(l)){l.dirty=true;return;}
        l.act.ib=l.eng.ib;
    }
    reinterpret_cast<void(STDMETHODCALLTYPE*)(L*,const D3D12_INDEX_BUFFER_VIEW*)>(ChainOf(self)->next[43])(self,view);
}
}

void Install(void** vtable,size_t lastSlot) {
    std::lock_guard lock(chainMutex);
    const auto n=chainCount.load(std::memory_order_relaxed);
    for(uint32_t i=0;i<n;++i)if(chains[i].vtable==vtable)return;
    if(n>=chains.size())return;
    auto& c=chains[n];
    c.vtable=vtable;
    for(size_t i=9;i<=lastSlot && i<kSlots;++i)c.next[i]=vtable[i];
    chainCount.store(n+1,std::memory_order_release);
    const auto put=[&](size_t slot,void* detour){if(slot<=lastSlot)InterlockedExchangePointer(reinterpret_cast<void* volatile*>(vtable+slot),detour);};
    for(const auto& f:kFlushSlots)put(f.slot,f.detour);
    put(11,reinterpret_cast<void*>(&ClearState));
    put(75,reinterpret_cast<void*>(&StateObject));
    put(20,reinterpret_cast<void*>(&Topology));
    put(30,reinterpret_cast<void*>(&Root));
    put(32,reinterpret_cast<void*>(&Table));
    put(43,reinterpret_cast<void*>(&IndexBuffer));
    put(46,reinterpret_cast<void*>(&Targets));
    put(23,reinterpret_cast<void*>(&BlendFactor));
    put(24,reinterpret_cast<void*>(&StencilRef));
    put(77,reinterpret_cast<void*>(&ShadingRate));
    put(78,reinterpret_cast<void*>(&ShadingImage));
    Log("[single-pass] command-list vtable %p chained (slots 9..%zu)\n",static_cast<void*>(vtable),lastSlot);
}
void Activate() {active.store(true,std::memory_order_release);}

void Reset(ID3D12GraphicsCommandList* list) {
    occ::Reset(list);
    if(!active.load(std::memory_order_relaxed))return;
    auto& l=Of(list);
    auto pending=std::move(l.pending);pending.clear();
    l=List{};l.pending=std::move(pending);   // keep the allocation
}
bool PipelineSet(ID3D12GraphicsCommandList* list,ID3D12PipelineState* engine,ID3D12PipelineState* bound) {
    if(!active.load(std::memory_order_relaxed))return false;
    auto& l=Of(list);l.engPso=engine;l.eng.pso=bound;
    if(Hold(l)){l.dirty=true;return true;}
    l.act.pso=bound;return false;
}
bool Viewports(ID3D12GraphicsCommandList* list,UINT count,const D3D12_VIEWPORT* vp) {
    if(!active.load(std::memory_order_relaxed))return false;
    auto& l=Of(list);const UINT n=vp?std::min<UINT>(count,D3D12_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE):0;
    l.eng.vpCount=n;if(n)std::memcpy(l.eng.vp,vp,sizeof(*vp)*n);
    if(Hold(l)){l.dirty=true;return true;}
    l.act.vpCount=n;if(n)std::memcpy(l.act.vp,vp,sizeof(*vp)*n);
    return false;
}
bool Scissors(ID3D12GraphicsCommandList* list,UINT count,const D3D12_RECT* sc) {
    if(!active.load(std::memory_order_relaxed))return false;
    auto& l=Of(list);const UINT n=sc?std::min<UINT>(count,D3D12_VIEWPORT_AND_SCISSORRECT_OBJECT_COUNT_PER_PIPELINE):0;
    l.eng.scCount=n;if(n)std::memcpy(l.eng.sc,sc,sizeof(*sc)*n);
    if(Hold(l)){l.dirty=true;return true;}
    l.act.scCount=n;if(n)std::memcpy(l.act.sc,sc,sizeof(*sc)*n);
    return false;
}
bool VertexBuffers(ID3D12GraphicsCommandList* list,UINT start,UINT count,const D3D12_VERTEX_BUFFER_VIEW* views) {
    if(!active.load(std::memory_order_relaxed))return false;
    auto& l=Of(list);
    for(UINT i=0;i<count && start+i<kVb;++i)l.eng.vb[start+i]=views?views[i]:D3D12_VERTEX_BUFFER_VIEW{};
    if(count && start<kVb)l.eng.vbUsed=std::max<uint32_t>(l.eng.vbUsed,std::min<uint32_t>(start+count,kVb));
    if(Hold(l) && start+count<=kVb){l.dirty=true;return true;}
    for(UINT i=0;i<count && start+i<kVb;++i)l.act.vb[start+i]=l.eng.vb[start+i];
    if(count && start<kVb)l.act.vbUsed=std::max<uint32_t>(l.act.vbUsed,std::min<uint32_t>(start+count,kVb));
    return false;
}
void Flush(ID3D12GraphicsCommandList* list) {
    if(!active.load(std::memory_order_relaxed))return;
    auto& l=Of(list);if(l.unsent)Cause(88);if(l.unsent || l.dirty)FlushList(list,l);
}

bool Draw(ID3D12GraphicsCommandList* list,uint32_t node,uint32_t kind,const uint32_t (&args)[5]) {
    if(!active.load(std::memory_order_relaxed))return false;
    auto& l=Of(list);
    const uint32_t mode=CyberpunkVR_SinglePass.load(std::memory_order_relaxed);
    const auto native=[&](size_t cause){if(l.unsent)Cause(cause);if(l.unsent || l.dirty)FlushList(list,l);return false;};
    const uint32_t wanted=CyberpunkVR_SinglePassNode.load(std::memory_order_relaxed);
    if(!mode || (wanted && node!=wanted) || kind<1 || kind>2){
        if(l.phase){l.phase=0;}
        return native(90);
    }
    // A batch never spans two nodes: whatever is kept goes before another node's draws.
    if(node!=l.node){
        if(l.unsent || l.dirty){if(l.unsent)Cause(90);FlushList(list,l);}
        l.phase=0;l.node=node;
    }
    const int eye=EyeOf(l);
    if(eye<0){Count(7)++;l.phase=0;return native(91);}
    const Hook* e=Originals(list);
    if(!e || !e->groot_original || !e->grootdt_original || !e->iaib_original || !e->topology_original)return native(89);
    if(eye==0) {
        if(l.phase!=1) {
            if(l.unsent){Cause(95);FlushList(list,l);}
            l.pending.clear();l.cursor=0;l.phase=1;l.broken=false;
            if((Count(8).fetch_add(1,std::memory_order_relaxed)+1)%4096==0 && cvr::RuntimeDiagnosticsEnabled()) {   // where kept draws went, now and then (a diagnostic)
                char line[1200];int u=0;
                for(size_t i=0;i<96 && u<640;++i)if(const auto n=CyberpunkVR_SinglePassFlushBy[i].load(std::memory_order_relaxed))
                    u+=snprintf(line+u,sizeof(line)-u," %zu:%llu",i,static_cast<unsigned long long>(n));
                u+=snprintf(line+u,sizeof(line)-u,"; pipelines registered %llu queued %llu built %llu failed %llu (%llu ms), "
                            "unknown replacement %llu, owned %llu MB",
                            static_cast<unsigned long long>(CyberpunkVR_SinglePassPipelineCounts[1].load()),
                            static_cast<unsigned long long>(CyberpunkVR_SinglePassPipelineCounts[2].load()),
                            static_cast<unsigned long long>(CyberpunkVR_SinglePassPipelineCounts[3].load()),
                            static_cast<unsigned long long>(CyberpunkVR_SinglePassPipelineCounts[4].load()),
                            static_cast<unsigned long long>(CyberpunkVR_SinglePassPipelineCounts[10].load()),
                            static_cast<unsigned long long>(CyberpunkVR_SinglePassPipelineCounts[11].load()),
                            static_cast<unsigned long long>(CyberpunkVR_SinglePassPipelineCounts[9].load()>>20));
                u+=snprintf(line+u,sizeof(line)-u,"; mismatch");
                for(size_t i=0;i<12 && u<1150;++i)u+=snprintf(line+u,sizeof(line)-u," %llu",
                    static_cast<unsigned long long>(CyberpunkVR_SinglePassMismatch[i].load(std::memory_order_relaxed)));
                Log("[single-pass] kept %llu, instanced %llu, flushed %llu; flushes by%s\n",static_cast<unsigned long long>(Count(0).load()),
                    static_cast<unsigned long long>(Count(1).load()),static_cast<unsigned long long>(Count(2).load()),u?line:" (none)");
            }
        }
        if(l.recPso!=l.engPso){l.rec=Find(l.engPso);l.recPso=l.engPso;}
        // Every left draw of the material root is kept. One whose twin is not ready (yet) is sent just in front of
        // its partner: the left eye's draws keep their order either way.
        // (without DXC no twin can come: then nothing is kept, the engine's draws go as they are)
        const bool material=l.rec && RootOf(l.rec)==l.eng.root && TwinRoot(l.eng.root) && (l.eng.mask & 1) && (l.eng.mask>>18 & 1) &&
                            (mode==2 || Available());
        if(!material) {
            if(l.unsent)Cause(92);
            if(l.unsent || l.dirty)FlushList(list,l);
            auto& p=l.pending.emplace_back();p.engine=l.engPso;p.sent=true;p.kind=kind;   // its partner goes as it is
            Count(4)++;
            return false;
        }
        auto& p=l.pending.emplace_back();   // in place: the state is ~700 bytes a draw
        p.engine=l.engPso;p.twin=mode==1?Twin(l.rec,l.engPso,l.eng.pso):nullptr;
        p.in=l.eng;p.vp=l.eng.vp[0];p.sc=l.eng.sc[0];p.kind=kind;
        std::memcpy(p.args,args,sizeof(p.args));
        ++l.unsent;Count(0)++;
        return true;
    }
    if(l.phase==1){l.phase=2;l.cursor=0;}
    if(l.phase!=2 || l.broken || l.cursor>=l.pending.size()){Count(3)++;return native(93);}
    Pending& p=l.pending[l.cursor++];
    if(p.sent){Count(3)++;return native(93);}
    // A partner that differs (its own vertex buffers, say): that left draw alone goes first, the next pairs go on.
    // Every earlier left draw was sent already, so the left eye's draws keep their order.
    if(!Matches(p,l,kind,args)){Count(6)++;Emit(list,e,l,p);Restore(list,l);return false;}
    if(mode==2 || !p.twin) {   // replay test, or no twin: the kept left draw just before its partner
        Emit(list,e,l,p);Restore(list,l);
        return false;
    }
    Instanced(list,e,l,p,kind,args);
    Count(1)++;
    return true;
}
}
