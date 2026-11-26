#include "Render/NativeGeometryLeases.hpp"
#include "Render/CommandResources.hpp"
#include "Render/RenderProbeScope.hpp"
#include "Render/NativeGeometrySnapshots.hpp"
#include <map>
#include <mutex>
#include <vector>
#include <stdexcept>
#include <algorithm>

#if !CVR_RUST_VRIK
extern "C" {
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GeometryLeaseSeq{},CyberpunkVR_GeometryLeaseCompatible{},CyberpunkVR_GeometryLeaseFailure{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_GeometryLeaseDrawCount{},CyberpunkVR_GeometryLeaseResourceCount{};
__declspec(dllexport) uint64_t CyberpunkVR_GeometryLeaseDraws{},CyberpunkVR_GeometryLeaseResources{};
__declspec(dllexport) uint64_t CyberpunkVR_GeometryLeaseReadStates{};
#endif
#if !CVR_RUST_VRIK
}
#endif
namespace cvr::stereo::geometry {
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
namespace {
// The bindings, the leases and the diagnostic rows: rust/crates/cvr-render/src/geometry_leases.rs (BufferLease and the
// rows mirrored there). The native draws, their instances and states, the freeze, the IA decoding and the exports stay
// here; the rows are published from Rust's tables.
struct RustPublished {uint64_t draws;uint32_t drawCount,resourceCount;uint64_t resources,readStates;};
struct RustSnapshot {uint32_t flags;bool freeze;DrawReadStates states;};
static_assert(sizeof(BufferLease)==80 && offsetof(BufferLease,native)==32 && sizeof(RustPublished)==32 && sizeof(RustSnapshot)==216 &&
    sizeof(native_probe::DrawRecord)==1096,"mirrored by geometry_leases.rs");
extern "C" {
void cvr_gl_begin(bool);
bool cvr_gl_flag(uint32_t);
void cvr_gl_bindings(uint32_t,ID3D12GraphicsCommandList*);
bool cvr_gl_index(ID3D12GraphicsCommandList*,const D3D12_INDEX_BUFFER_VIEW*,const BufferLease*);
uint32_t cvr_gl_vertices(ID3D12GraphicsCommandList*,UINT,UINT,const D3D12_VERTEX_BUFFER_VIEW*,const BufferLease*);
uint32_t cvr_gl_snapshot(const native_probe::DrawRecord*,size_t,size_t,bool,bool,void(*)(void*,const BufferLease*,ReadState*),BufferLease*,BufferLease*,
    RustSnapshot*);
bool cvr_gl_copy_draw(uint32_t,BufferLease*,BufferLease*,size_t*);
bool cvr_gl_current(ID3D12GraphicsCommandList*,const native_probe::DrawRecord*,bool,bool,BufferLease*,BufferLease*);
void cvr_gl_published(RustPublished*);
}
std::mutex mutex;
std::vector<native_probe::DrawRecord> nativeDraws;
std::vector<std::vector<uint8_t>> instanceData;
std::vector<scene_state::DrawState> drawStates;
size_t instanceBytes{};
void Publish() {
    RustPublished p{};cvr_gl_published(&p);
    CyberpunkVR_GeometryLeaseDraws=p.draws;CyberpunkVR_GeometryLeaseDrawCount=p.drawCount;
    CyberpunkVR_GeometryLeaseResources=p.resources;CyberpunkVR_GeometryLeaseResourceCount=p.resourceCount;
    CyberpunkVR_GeometryLeaseReadStates=p.readStates;
}
void ReadAtDraw(void* list,const BufferLease* lease,ReadState* out){*out=ReadStateAtDraw(static_cast<ID3D12GraphicsCommandList*>(list),*lease);}
}
void Begin(bool enabled) {
    std::lock_guard lock(mutex);CyberpunkVR_GeometryLeaseSeq.fetch_add(1);
    cvr_gl_begin(enabled);CyberpunkVR_GeometryLeaseFailure=0;
    geometry_snapshots::Begin(enabled && CyberpunkVR_GeometryFreezeEnable.load()!=0);
    nativeDraws.clear();instanceData.clear();drawStates.clear();instanceBytes=0;
    if(enabled)CyberpunkVR_GeometryLeaseCompatible=NativeIaCompatible()?1:2;
    Publish();CyberpunkVR_GeometryLeaseSeq.fetch_add(1);
}
void BeginBindings(){cvr_gl_bindings(0,nullptr);}
void EndBindings(){cvr_gl_bindings(1,nullptr);}
void End(){cvr_gl_bindings(2,nullptr);}
void ClearBindings(){cvr_gl_bindings(3,nullptr);}
void Reset(ID3D12GraphicsCommandList* list){cvr_gl_bindings(4,list);}
void Index(ID3D12GraphicsCommandList* list,const D3D12_INDEX_BUFFER_VIEW* view) {
    if(!cvr_gl_flag(0) || probe::internalCommands || !list)return;
    IndexLease(list,view,view?BoundIndex(*view):BufferLease{});
}
void IndexLease(ID3D12GraphicsCommandList* list,const D3D12_INDEX_BUFFER_VIEW* view,BufferLease lease) {
    if(probe::internalCommands || !list)return;
    if(!cvr_gl_index(list,view,&lease))CyberpunkVR_GeometryLeaseFailure=1;
}
void Vertices(ID3D12GraphicsCommandList* list,UINT start,UINT count,const D3D12_VERTEX_BUFFER_VIEW* views) {
    if(!cvr_gl_flag(0) || probe::internalCommands || !list)return;
    if(start>32 || count>32-start){CyberpunkVR_GeometryLeaseFailure=2;return;}
    if(start>=16)return;
    VertexLeases(list,start,count,views,BoundVertices(start,count,views));
}
void VertexLeases(ID3D12GraphicsCommandList* list,UINT start,UINT count,const D3D12_VERTEX_BUFFER_VIEW* views,std::array<BufferLease,16> leases) {
    if(probe::internalCommands || !list)return;
    if(const auto failure=cvr_gl_vertices(list,start,count,views,leases.data()))CyberpunkVR_GeometryLeaseFailure=failure;
}
void Snapshot(const native_probe::DrawRecord& draw,std::span<const uint8_t> instances,const scene_state::DrawState* drawState,bool freeze) {
    if(!cvr_gl_flag(1))return;
    std::lock_guard lock(mutex);if(!cvr_gl_flag(1))return;
    CyberpunkVR_GeometryLeaseSeq.fetch_add(1);
    try {
        const bool stateOk=drawState && reinterpret_cast<uintptr_t>(drawState->original.Get())==draw.pipeline && drawState->topology && drawState->rate.known;
        DrawResources source;RustSnapshot out{};   // the freeze's leases are given into source
        const auto result=cvr_gl_snapshot(&draw,instances.size(),instanceBytes,stateOk,freeze && CyberpunkVR_GeometryFreezeEnable.load(std::memory_order_relaxed),
            &ReadAtDraw,&source.index,source.vertices.data(),&out);
        if(result==1)CyberpunkVR_GeometryLeaseFailure=1;
        else if(!result){
            if(out.freeze){source.draw=draw;source.state=*drawState;geometry_snapshots::Capture(source,out.states.buffers,true);}
            drawStates.push_back((out.flags&16)?*drawState:scene_state::DrawState{});
            instanceData.emplace_back(instances.begin(),instances.end());instanceBytes+=instances.size();nativeDraws.push_back(draw);
        }
    }catch(...){CyberpunkVR_GeometryLeaseFailure=1;}
    Publish();CyberpunkVR_GeometryLeaseSeq.fetch_add(1);
}
bool CopyDraw(uint32_t sequence,DrawResources& output) {
    std::lock_guard lock(mutex);if(CyberpunkVR_GeometryLeaseFailure.load())return false;
    DrawResources result;size_t at{};
    if(!cvr_gl_copy_draw(sequence,&result.index,result.vertices.data(),&at))return false;   // the owned leases, given
    result.draw=nativeDraws[at];result.instances=instanceData[at];result.state=drawStates[at];
    output=std::move(result);return true;
}
bool CopyCurrentBindings(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw,bool indexed,DrawResources& output)try{
    DrawResources pending;pending.draw=draw;
    if(!cvr_gl_current(list,&draw,indexed,probe::internalCommands,&pending.index,pending.vertices.data()))return false;   // the exact bound leases, given
    output=std::move(pending);return true;
}catch(...){return false;}
#endif
#else
namespace {
struct State {
    BufferLease index;D3D12_INDEX_BUFFER_VIEW indexView{};
    std::array<BufferLease,16> vertices;std::array<D3D12_VERTEX_BUFFER_VIEW,16> views{};
};
std::mutex mutex;std::atomic<bool> active{},snapshotting{};
std::map<ID3D12GraphicsCommandList*,State> states;
std::vector<DrawLeases> draws;
std::vector<ResourceRecord> resources;
std::vector<BufferLease> owners;
std::vector<native_probe::DrawRecord> nativeDraws;
std::vector<std::vector<uint8_t>> instanceData;
std::vector<scene_state::DrawState> drawStates;
std::vector<DrawReadStates> readStates;
size_t instanceBytes{};
std::map<ID3D12Resource*,UINT> ids;
bool Matches(const BufferLease& lease,uint64_t address,uint64_t bytes) {
    return lease.resource && bytes && address>=lease.gpu && address-lease.gpu<=lease.bytes && bytes<=lease.bytes-(address-lease.gpu);
}
UINT Own(const BufferLease& lease) {
    if(!lease.resource)return 0;
    if(const auto found=ids.find(lease.resource.Get());found!=ids.end())return found->second;
    if(resources.size()>=8192)throw std::length_error("geometry resource lease limit");
    const auto id=UINT(resources.size());owners.push_back(lease);
    resources.push_back({reinterpret_cast<uintptr_t>(lease.resource.Get()),lease.gpu,lease.bytes,lease.heapType,lease.flags});
    ids.emplace(lease.resource.Get(),id);return id;
}
void Publish() {
    CyberpunkVR_GeometryLeaseDraws=reinterpret_cast<uintptr_t>(draws.data());CyberpunkVR_GeometryLeaseDrawCount=UINT(draws.size());
    CyberpunkVR_GeometryLeaseResources=reinterpret_cast<uintptr_t>(resources.data());CyberpunkVR_GeometryLeaseResourceCount=UINT(resources.size());
    CyberpunkVR_GeometryLeaseReadStates=reinterpret_cast<uintptr_t>(readStates.data());
}
}
void Begin(bool enabled) {
    std::lock_guard lock(mutex);CyberpunkVR_GeometryLeaseSeq.fetch_add(1);
    active=enabled;snapshotting=enabled;states.clear();draws.clear();resources.assign(1,{});owners.assign(1,{});ids.clear();CyberpunkVR_GeometryLeaseFailure=0;
    geometry_snapshots::Begin(enabled && CyberpunkVR_GeometryFreezeEnable.load()!=0);
    nativeDraws.clear();instanceData.clear();drawStates.clear();readStates.clear();instanceBytes=0;
    if(enabled)CyberpunkVR_GeometryLeaseCompatible=NativeIaCompatible()?1:2;
    Publish();CyberpunkVR_GeometryLeaseSeq.fetch_add(1);
}
void BeginBindings(){std::lock_guard lock(mutex);snapshotting=false;active=true;states.clear();}
void EndBindings(){std::lock_guard lock(mutex);if(!snapshotting.load()){active=false;states.clear();}}
void End(){active=false;snapshotting=false;ClearBindings();}
void ClearBindings(){std::lock_guard lock(mutex);states.clear();}
void Reset(ID3D12GraphicsCommandList* list){if(!active.load())return;std::lock_guard lock(mutex);states.erase(list);}
void Index(ID3D12GraphicsCommandList* list,const D3D12_INDEX_BUFFER_VIEW* view) {
    if(!active.load(std::memory_order_relaxed) || probe::internalCommands || !list)return;
    IndexLease(list,view,view?BoundIndex(*view):BufferLease{});
}
void IndexLease(ID3D12GraphicsCommandList* list,const D3D12_INDEX_BUFFER_VIEW* view,BufferLease lease) {
    if(!active.load(std::memory_order_relaxed) || probe::internalCommands || !list)return;
    try {
        std::lock_guard lock(mutex);
        if(!active.load(std::memory_order_relaxed))return;
        if(states.size()>=512 && !states.contains(list))throw std::length_error("geometry state limit");
        auto& state=states[list];state.index=std::move(lease);state.indexView=view?*view:D3D12_INDEX_BUFFER_VIEW{};
    }catch(...){CyberpunkVR_GeometryLeaseFailure=1;}
}
void Vertices(ID3D12GraphicsCommandList* list,UINT start,UINT count,const D3D12_VERTEX_BUFFER_VIEW* views) {
    if(!active.load(std::memory_order_relaxed) || probe::internalCommands || !list)return;
    if(start>32 || count>32-start){CyberpunkVR_GeometryLeaseFailure=2;return;}
    if(start>=16)return;
    VertexLeases(list,start,count,views,BoundVertices(start,count,views));
}
void VertexLeases(ID3D12GraphicsCommandList* list,UINT start,UINT count,const D3D12_VERTEX_BUFFER_VIEW* views,std::array<BufferLease,16> leases) {
    if(!active.load(std::memory_order_relaxed) || probe::internalCommands || !list)return;
    if(start>32 || count>32-start){CyberpunkVR_GeometryLeaseFailure=2;return;}
    if(start>=16)return;
    count=std::min(count,16-start);
    try {
        std::lock_guard lock(mutex);
        if(!active.load(std::memory_order_relaxed))return;
        if(states.size()>=512 && !states.contains(list))throw std::length_error("geometry state limit");
        auto& state=states[list];for(UINT i=0;i<count;++i) {
            state.vertices[start+i]=std::move(leases[i]);state.views[start+i]=views?views[i]:D3D12_VERTEX_BUFFER_VIEW{};
        }
    }catch(...){CyberpunkVR_GeometryLeaseFailure=1;}
}
void Snapshot(const native_probe::DrawRecord& draw,std::span<const uint8_t> instances,const scene_state::DrawState* drawState,bool freeze) {
    if(!snapshotting.load(std::memory_order_relaxed))return;
    std::lock_guard lock(mutex);if(!snapshotting.load(std::memory_order_relaxed))return;
    CyberpunkVR_GeometryLeaseSeq.fetch_add(1);
    try {
        if(draws.size()>=native_probe::DrawCapacity)throw std::length_error("geometry draw limit");
        DrawLeases row;row.sequence=UINT(draw.sequence);row.usedMask=draw.inputSlotMask;row.frame=draw.frame;row.side=draw.side;
        DrawReadStates statesAtDraw;statesAtDraw.sequence=row.sequence;
        const auto found=states.find(reinterpret_cast<ID3D12GraphicsCommandList*>(draw.list));
        if(found!=states.end()) {
            const auto& state=found->second;
            auto* list=reinterpret_cast<ID3D12GraphicsCommandList*>(draw.list);
            statesAtDraw.buffers[0]=ReadStateAtDraw(list,state.index);
            for(UINT slot=0;slot<16;++slot)if(row.usedMask&(1u<<slot))statesAtDraw.buffers[slot+1]=ReadStateAtDraw(list,state.vertices[slot]);
            const auto indexSize=draw.index.Format==DXGI_FORMAT_R16_UINT?2u:draw.index.Format==DXGI_FORMAT_R32_UINT?4u:0u;
            if(indexSize && draw.indices && uint64_t(draw.firstIndex)*indexSize<=draw.index.SizeInBytes &&
               uint64_t(draw.indices)*indexSize<=draw.index.SizeInBytes-uint64_t(draw.firstIndex)*indexSize &&
               !std::memcmp(&draw.index,&state.indexView,sizeof(draw.index)) && Matches(state.index,draw.index.BufferLocation,draw.index.SizeInBytes)) {
                row.index=Own(state.index);row.flags|=1;
            }
            if((draw.flags&128) && !(draw.inputSlotMask&0xFFFF0000u))row.flags|=2;
            for(UINT slot=0;slot<16;++slot)if(draw.inputSlotMask&(1u<<slot)) {
                const auto& view=draw.vertices[slot];
                if(!std::memcmp(&view,&state.views[slot],sizeof(view)) && Matches(state.vertices[slot],view.BufferLocation,view.SizeInBytes)) {
                    row.vertices[slot]=Own(state.vertices[slot]);row.ownedMask|=1u<<slot;
                }
            }
            if((row.flags&2) && row.ownedMask==row.usedMask)row.flags|=4;
            if(row.flags==7) {
                bool kept=gpu::KeepCommandResources(reinterpret_cast<ID3D12GraphicsCommandList*>(draw.list),{state.index.resource.Get()});
                for(UINT slot=0;slot<16;++slot)if(row.usedMask&(1u<<slot))kept&=gpu::KeepCommandResources(reinterpret_cast<ID3D12GraphicsCommandList*>(draw.list),{state.vertices[slot].resource.Get()});
                if(kept)row.flags|=8;
            }
        }
        if(instances.size()>16*1024 || instanceBytes+instances.size()>32*1024*1024)throw std::length_error("geometry instance snapshot limit");
        if(drawState && reinterpret_cast<uintptr_t>(drawState->original.Get())==draw.pipeline && drawState->topology && drawState->rate.known)row.flags|=16;
        if(freeze && row.flags==31 && CyberpunkVR_GeometryFreezeEnable.load(std::memory_order_relaxed)) {
            DrawResources source;source.index=owners[row.index];source.draw=draw;source.state=*drawState;
            for(UINT slot=0;slot<16;++slot)if(row.usedMask&(1u<<slot))source.vertices[slot]=owners[row.vertices[slot]];
            geometry_snapshots::Capture(source,statesAtDraw.buffers,true);
        }
        drawStates.push_back((row.flags&16)?*drawState:scene_state::DrawState{});
        readStates.push_back(statesAtDraw);
        instanceData.emplace_back(instances.begin(),instances.end());instanceBytes+=instances.size();nativeDraws.push_back(draw);draws.push_back(row);
    }catch(...){CyberpunkVR_GeometryLeaseFailure=1;}
    Publish();CyberpunkVR_GeometryLeaseSeq.fetch_add(1);
}
bool CopyDraw(uint32_t sequence,DrawResources& output) {
    std::lock_guard lock(mutex);if(CyberpunkVR_GeometryLeaseFailure.load())return false;
    const auto found=std::find_if(draws.begin(),draws.end(),[&](const auto& row){return row.sequence==sequence && (row.flags&15)==15;});
    if(found==draws.end() || !found->index || found->index>=owners.size())return false;
    DrawResources result;result.index=owners[found->index];
    const auto at=size_t(found-draws.begin());result.draw=nativeDraws[at];result.instances=instanceData[at];result.state=drawStates[at];
    for(UINT slot=0;slot<16;++slot)if(found->usedMask&(1u<<slot)) {
        if(!found->vertices[slot] || found->vertices[slot]>=owners.size())return false;result.vertices[slot]=owners[found->vertices[slot]];
    }
    output=std::move(result);return true;
}
bool CopyCurrentBindings(ID3D12GraphicsCommandList* list,const native_probe::DrawRecord& draw,bool indexed,DrawResources& output)try{
    if(!list || draw.list!=reinterpret_cast<uintptr_t>(list) || probe::internalCommands || !active.load(std::memory_order_relaxed) ||
        !(draw.flags&128) || (draw.inputSlotMask&~65535u))return false;
    std::lock_guard lock(mutex);const auto found=states.find(list);
    if(!active.load(std::memory_order_relaxed) || found==states.end())return false;
    const auto& state=found->second;DrawResources pending;pending.draw=draw;
    if(indexed){
        const auto bytes=draw.index.Format==DXGI_FORMAT_R16_UINT?2u:draw.index.Format==DXGI_FORMAT_R32_UINT?4u:0u;
        if(!bytes || draw.index.BufferLocation%bytes || draw.index.SizeInBytes%bytes ||
            std::memcmp(&draw.index,&state.indexView,sizeof(draw.index)) ||
            !Matches(state.index,draw.index.BufferLocation,draw.index.SizeInBytes))return false;
        pending.index=state.index;
    }
    for(UINT slot=0;slot<16;++slot)if(draw.inputSlotMask&(1u<<slot)){
        const auto& view=draw.vertices[slot];
        if(std::memcmp(&view,&state.views[slot],sizeof(view)) || !Matches(state.vertices[slot],view.BufferLocation,view.SizeInBytes))return false;
        pending.vertices[slot]=state.vertices[slot];
    }
    output=std::move(pending);return true;
}catch(...){return false;}
#endif
}
