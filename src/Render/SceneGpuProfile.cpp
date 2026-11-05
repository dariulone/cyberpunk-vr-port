#include "Render/SceneGpuProfile.hpp"
#include "Render/CommandResources.hpp"
#include <wrl/client.h>
#include <algorithm>
#include <array>
#include <chrono>
#include <cstring>
#include <map>
#include <mutex>
#include <sstream>
#include <tuple>
#include <unordered_map>
#include <set>

extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SceneGpuRequest{0},CyberpunkVR_SceneGpuReportSeq{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SceneGpuGroupMode{0},CyberpunkVR_SceneGpuNodeFilter{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SceneGpuViewportDetail{0};
// Frame mode (tools/vr_bench): profile every command list whose recording starts in the next N whole frames (1..32),
// counted by the Present hook. A list begun in the frame the request lands in is left out (it is a partial frame),
// so the report's sums divided by "frames" are per-frame times. Takes precedence over CyberpunkVR_SceneGpuRequest.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SceneGpuFrameRequest{0};
__declspec(dllexport) char CyberpunkVR_SceneGpuReport[262144]="{\"active\":false}";
// Diagnostic: 1 = every range on a direct list also gets a pipeline-statistics query (IA / VS / clipper / PS counts,
// D3D12_QUERY_DATA_PIPELINE_STATISTICS), summed per group as "ps" in the report. Read when a profile starts.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SceneGpuPipelineStats{0};
// Diagnostic: 1 = every draw of a profiled range on a direct list gets an occlusion query; the report's "occ" counts
// per eye (viewport kind 1 left / whole frame, 2 right half, 3 other) the draws and how many samples passed:
// [draws, 0, 1-64, 65-4096, more]. Read when a profile starts.
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SceneGpuDrawOcclusion{0};
// Present count (Hooks/SwapChain.cpp): [0] counts Presents.
extern std::atomic<uint64_t> CyberpunkVR_PresentTiming[8];
}
namespace cvr::gpu::scene_profile {
using Microsoft::WRL::ComPtr;
std::atomic<bool> active{false};
namespace {
// A list is recorded by one thread at a time, so the last viewport of the list this thread records is enough.
thread_local ID3D12GraphicsCommandList* t_viewportList{};
thread_local uint32_t t_viewportDetail{};
}
void Viewport(ID3D12GraphicsCommandList* list,UINT count,const D3D12_VIEWPORT* viewports) {
    if(!active.load(std::memory_order_relaxed) || !CyberpunkVR_SceneGpuViewportDetail.load(std::memory_order_relaxed))return;
    if(!count || !viewports){t_viewportList=nullptr;return;}
    const auto x=uint32_t(viewports[0].TopLeftX+.5f),width=uint32_t(viewports[0].Width+.5f);
    const uint32_t kind=x==0 ? 1u : ((x>width ? x-width : width-x)<=1 ? 2u : 3u);
    t_viewportList=list;t_viewportDetail=0x100000u|(kind<<16)|(width&0xFFFFu);
}
uint32_t DrawDetail(ID3D12GraphicsCommandList* list) {
    return t_viewportList==list && CyberpunkVR_SceneGpuViewportDetail.load(std::memory_order_relaxed) ? t_viewportDetail : 0;
}
// Ranges per list, and lists per profile: one query heap and one readback buffer per profile, a slice of each per
// list (a committed readback buffer per list cost a 64 KiB allocation each).
constexpr unsigned Capacity=256;
constexpr unsigned PoolLists=1536;
constexpr unsigned StatsCapacity=64;   // pipeline-statistics ranges per list (88 bytes each in the readback)
constexpr unsigned StatsCount=11;      // UINT64s in D3D12_QUERY_DATA_PIPELINE_STATISTICS
constexpr unsigned DrawCapacity=2048;  // occlusion-queried draws per list
struct Key {
    uint32_t node{};int side{};uint64_t pso{};uint32_t detail{};
    auto Fields() const {return std::tie(node,side,pso,detail);}
    bool operator==(const Key& other) const {return Fields()==other.Fields();}
    bool operator<(const Key& other) const {return Fields()<other.Fields();}
};
struct Pool {
    ComPtr<ID3D12QueryHeap> heap;
    ComPtr<ID3D12Resource> readback;
    ComPtr<ID3D12QueryHeap> statsHeap;      // only in pipeline-statistics mode
    ComPtr<ID3D12Resource> statsReadback;
    ComPtr<ID3D12QueryHeap> occHeap;        // only in draw-occlusion mode
    ComPtr<ID3D12Resource> occReadback;
    ID3D12Device* device{};
    unsigned next{};
};
struct Batch {
    std::shared_ptr<Pool> pool;
    unsigned slice{};
    ComPtr<ID3D12Fence> fence;
    uint64_t value{},frequency{};
    struct Range {Key key;uint32_t commands{};int stat=-1;};
    std::vector<Range> ranges;
    unsigned statsUsed{};
    std::vector<uint8_t> drawKind;          // per occlusion-queried draw: the viewport kind (1, 2, 3; 0 unknown)
    std::vector<uint32_t> drawRange;        // ... and the range (key) it was drawn in
    bool drawOpen{};
    bool closed{},claimed{},invalid{},rangeOpen{},compute{},statOpen{};
    unsigned Query(size_t index) const {return slice*Capacity*2+unsigned(index);}
    unsigned StatQuery(unsigned index) const {return slice*StatsCapacity+index;}
    unsigned DrawQuery(size_t index) const {return slice*DrawCapacity+unsigned(index);}
};
namespace {
std::mutex mutex;
unsigned remaining{},completed{},discarded{};
bool frameMode{},exhausted{};
uint64_t startSerial{},endSerial{};
std::chrono::steady_clock::time_point deadline;
std::shared_ptr<Pool> pool;
std::unordered_map<ID3D12GraphicsCommandList*,std::shared_ptr<Batch>> lists;
std::vector<std::shared_ptr<Batch>> pending;
struct Statistic {unsigned ranges{},commands{},lists{};double sumMs{},maxMs{};std::array<uint64_t,StatsCount> stats{};bool hasStats{};
                  std::array<uint64_t,5> occ{};};
bool statsMode{},occMode{};
std::array<std::array<uint64_t,5>,4> occCounts{};   // by viewport kind: draws, 0, 1-64, 65-4096, more samples
std::map<Key,Statistic> statistics;
uint64_t Serial() {return CyberpunkVR_PresentTiming[0].load(std::memory_order_relaxed);}
void Publish() {
    std::ostringstream out;
    out<<"{\"active\":"<<(active.load()?"true":"false")<<",\"remaining\":"<<remaining
       <<",\"pending\":"<<pending.size()<<",\"recordings\":"<<lists.size()
       <<",\"completed\":"<<completed<<",\"discarded\":"<<discarded
       <<",\"mode\":\""<<(frameMode?"frames":"lists")<<"\",\"frames\":"<<(frameMode?endSerial-startSerial:0)
       <<",\"exhausted\":"<<(exhausted?"true":"false")<<",\"groups\":[";
    bool first=true;
    for(const auto& [key,value]:statistics) {
        if(!first)out<<',';first=false;
        out<<"{\"node\":"<<key.node<<",\"side\":"<<key.side<<",\"pso\":"<<key.pso
           <<",\"ranges\":"<<value.ranges<<",\"commands\":"<<value.commands<<",\"lists\":"<<value.lists<<",\"detail\":"<<key.detail
           <<",\"sum_ms\":"<<value.sumMs<<",\"max_ms\":"<<value.maxMs;
        if(value.hasStats){out<<",\"ps\":[";for(unsigned i=0;i<StatsCount;++i)out<<(i?",":"")<<value.stats[i];out<<']';}
        if(value.occ[0]){out<<",\"occ\":[";for(unsigned i=0;i<5;++i)out<<(i?",":"")<<value.occ[i];out<<']';}
        out<<'}';
    }
    out<<"]";
    if(occMode) {
        out<<",\"occ\":{";
        for(unsigned k=0;k<4;++k) {
            out<<(k?",":"")<<'"'<<k<<"\":[";
            for(unsigned b=0;b<5;++b)out<<(b?",":"")<<occCounts[k][b];
            out<<']';
        }
        out<<'}';
    }
    out<<'}';auto text=out.str();
    if(text.size()>=sizeof(CyberpunkVR_SceneGpuReport))text="{\"error\":\"report overflow\"}";
    CyberpunkVR_SceneGpuReportSeq.fetch_add(1,std::memory_order_acq_rel);
    std::memcpy(CyberpunkVR_SceneGpuReport,text.c_str(),text.size()+1);
    CyberpunkVR_SceneGpuReportSeq.fetch_add(1,std::memory_order_release);
}
std::shared_ptr<Pool> MakePool(ID3D12Device* device) {
    auto value=std::make_shared<Pool>();value->device=device;
    D3D12_QUERY_HEAP_DESC query{};query.Type=D3D12_QUERY_HEAP_TYPE_TIMESTAMP;query.Count=PoolLists*Capacity*2;
    D3D12_RESOURCE_DESC desc{};desc.Dimension=D3D12_RESOURCE_DIMENSION_BUFFER;desc.Width=UINT64(PoolLists)*Capacity*16;
    desc.Height=desc.DepthOrArraySize=desc.MipLevels=1;desc.SampleDesc.Count=1;desc.Layout=D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES heap{};heap.Type=D3D12_HEAP_TYPE_READBACK;
    if(FAILED(device->CreateQueryHeap(&query,IID_PPV_ARGS(&value->heap))) ||
       FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&value->readback))))
        return {};
    if(occMode) {
        D3D12_QUERY_HEAP_DESC occ{};occ.Type=D3D12_QUERY_HEAP_TYPE_OCCLUSION;occ.Count=PoolLists*DrawCapacity;
        D3D12_RESOURCE_DESC buffer=desc;buffer.Width=UINT64(PoolLists)*DrawCapacity*8;
        if(FAILED(device->CreateQueryHeap(&occ,IID_PPV_ARGS(&value->occHeap))) ||
           FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&buffer,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&value->occReadback))))
            return {};
    }
    if(statsMode) {
        D3D12_QUERY_HEAP_DESC stats{};stats.Type=D3D12_QUERY_HEAP_TYPE_PIPELINE_STATISTICS;stats.Count=PoolLists*StatsCapacity;
        desc.Width=UINT64(PoolLists)*StatsCapacity*sizeof(D3D12_QUERY_DATA_PIPELINE_STATISTICS);
        if(FAILED(device->CreateQueryHeap(&stats,IID_PPV_ARGS(&value->statsHeap))) ||
           FAILED(device->CreateCommittedResource(&heap,D3D12_HEAP_FLAG_NONE,&desc,D3D12_RESOURCE_STATE_COPY_DEST,nullptr,IID_PPV_ARGS(&value->statsReadback))))
            return {};
    }
    return value;
}
std::shared_ptr<Batch> Create(ID3D12GraphicsCommandList* list) {
    const auto type=list->GetType();
    if(type!=D3D12_COMMAND_LIST_TYPE_DIRECT && type!=D3D12_COMMAND_LIST_TYPE_COMPUTE)return {};
    ComPtr<ID3D12Device> device;if(FAILED(list->GetDevice(IID_PPV_ARGS(&device))))return {};
    if(!pool || pool->device!=device.Get()) {pool=MakePool(device.Get());if(!pool)return {};}
    if(pool->next>=PoolLists){exhausted=true;remaining=0;return {};}
    auto batch=std::make_shared<Batch>();batch->ranges.reserve(Capacity);
    batch->pool=pool;batch->slice=pool->next++;batch->compute=type==D3D12_COMMAND_LIST_TYPE_COMPUTE;
    if(!KeepCommandResources(list,{pool->heap.Get(),pool->readback.Get()}))return {};
    if(pool->statsHeap && !KeepCommandResources(list,{pool->statsHeap.Get(),pool->statsReadback.Get()}))return {};
    if(pool->occHeap && !KeepCommandResources(list,{pool->occHeap.Get(),pool->occReadback.Get()}))return {};
    if(pool->occHeap){batch->drawKind.reserve(256);batch->drawRange.reserve(256);}
    return batch;
}
}
namespace {
void EndStat(ID3D12GraphicsCommandList* list,Batch& batch) {
    if(!batch.statOpen)return;
    list->EndQuery(batch.pool->statsHeap.Get(),D3D12_QUERY_TYPE_PIPELINE_STATISTICS,batch.StatQuery(batch.statsUsed-1));
    batch.statOpen=false;
}
}
void Mark(ID3D12GraphicsCommandList* list,uint32_t node,int side,ID3D12PipelineState* pso,uint32_t detail) {
    if(!active.load(std::memory_order_relaxed) || !list)return;
    const auto filter=CyberpunkVR_SceneGpuNodeFilter.load(std::memory_order_relaxed);
    if(filter && filter!=node){EndRange(list);return;}
    std::lock_guard lock(mutex);
    auto it=lists.find(list);
    if(it==lists.end()) {
        if(!remaining || !node || side<0 || side>2)return;
        if(frameMode) {
            // Only lists begun inside the whole frames asked for.
            const auto serial=Serial();
            if(serial<=startSerial)return;
            if(serial>endSerial){remaining=0;return;}
        } else --remaining;
        auto batch=Create(list);if(!batch){++discarded;return;}
        it=lists.emplace(list,std::move(batch)).first;
    }
    auto& batch=*it->second;
    if(batch.closed || batch.invalid)return;
    auto* heap=batch.pool->heap.Get();
    // 0x200000: recorded on a compute list (async compute: its span overlaps the graphics queue's work).
    Key key{node,side,CyberpunkVR_SceneGpuGroupMode.load(std::memory_order_relaxed)?0:reinterpret_cast<uint64_t>(pso),detail|(batch.compute?0x200000u:0u)};
    if(batch.rangeOpen && batch.ranges.back().key==key){++batch.ranges.back().commands;return;}
    if(batch.rangeOpen)list->EndQuery(heap,D3D12_QUERY_TYPE_TIMESTAMP,batch.Query(batch.ranges.size()*2-1));
    EndStat(list,batch);
    if(batch.ranges.size()==Capacity){batch.invalid=true;return;}
    list->EndQuery(heap,D3D12_QUERY_TYPE_TIMESTAMP,batch.Query(batch.ranges.size()*2));
    batch.ranges.push_back({key,1});
    batch.rangeOpen=true;
    if(batch.pool->statsHeap && !batch.compute && batch.statsUsed<StatsCapacity) {
        list->BeginQuery(batch.pool->statsHeap.Get(),D3D12_QUERY_TYPE_PIPELINE_STATISTICS,batch.StatQuery(batch.statsUsed));
        batch.ranges.back().stat=int(batch.statsUsed++);batch.statOpen=true;
    }
}
void DrawBegin(ID3D12GraphicsCommandList* list) {
    if(!active.load(std::memory_order_relaxed) || !occMode)return;
    std::lock_guard lock(mutex);const auto it=lists.find(list);if(it==lists.end())return;auto& batch=*it->second;
    // Only inside a profiled range (the node filter ends the range for other nodes).
    if(batch.closed || batch.invalid || !batch.rangeOpen || batch.compute || batch.drawOpen || !batch.pool->occHeap ||
       batch.drawKind.size()>=DrawCapacity)return;
    const uint32_t detail=DrawDetail(list);
    batch.drawKind.push_back(detail&0x100000u ? uint8_t((detail>>16)&3u) : uint8_t(0));
    batch.drawRange.push_back(uint32_t(batch.ranges.size()-1));
    list->BeginQuery(batch.pool->occHeap.Get(),D3D12_QUERY_TYPE_OCCLUSION,batch.DrawQuery(batch.drawKind.size()-1));
    batch.drawOpen=true;
}
void DrawEnd(ID3D12GraphicsCommandList* list) {
    if(!active.load(std::memory_order_relaxed) || !occMode)return;
    std::lock_guard lock(mutex);const auto it=lists.find(list);if(it==lists.end())return;auto& batch=*it->second;
    if(!batch.drawOpen)return;
    list->EndQuery(batch.pool->occHeap.Get(),D3D12_QUERY_TYPE_OCCLUSION,batch.DrawQuery(batch.drawKind.size()-1));
    batch.drawOpen=false;
}
void EndRange(ID3D12GraphicsCommandList* list) {
    if(!active.load(std::memory_order_relaxed))return;
    std::lock_guard lock(mutex);const auto it=lists.find(list);if(it==lists.end())return;auto& batch=*it->second;
    if(batch.closed || batch.invalid || !batch.rangeOpen)return;
    list->EndQuery(batch.pool->heap.Get(),D3D12_QUERY_TYPE_TIMESTAMP,batch.Query(batch.ranges.size()*2-1));batch.rangeOpen=false;
    EndStat(list,batch);
}
void Close(ID3D12GraphicsCommandList* list) {
    if(!active.load(std::memory_order_relaxed))return;
    std::lock_guard lock(mutex);const auto it=lists.find(list);if(it==lists.end())return;
    auto& batch=*it->second;if(batch.closed || batch.invalid || batch.ranges.empty())return;
    auto* heap=batch.pool->heap.Get();
    if(batch.rangeOpen)list->EndQuery(heap,D3D12_QUERY_TYPE_TIMESTAMP,batch.Query(batch.ranges.size()*2-1));
    batch.rangeOpen=false;
    EndStat(list,batch);
    list->ResolveQueryData(heap,D3D12_QUERY_TYPE_TIMESTAMP,batch.Query(0),unsigned(batch.ranges.size()*2),
                           batch.pool->readback.Get(),UINT64(batch.slice)*Capacity*16);
    if(batch.drawOpen){list->EndQuery(batch.pool->occHeap.Get(),D3D12_QUERY_TYPE_OCCLUSION,batch.DrawQuery(batch.drawKind.size()-1));batch.drawOpen=false;}
    if(!batch.drawKind.empty())
        list->ResolveQueryData(batch.pool->occHeap.Get(),D3D12_QUERY_TYPE_OCCLUSION,batch.DrawQuery(0),unsigned(batch.drawKind.size()),
                               batch.pool->occReadback.Get(),UINT64(batch.slice)*DrawCapacity*8);
    if(batch.statsUsed)
        list->ResolveQueryData(batch.pool->statsHeap.Get(),D3D12_QUERY_TYPE_PIPELINE_STATISTICS,batch.StatQuery(0),batch.statsUsed,
                               batch.pool->statsReadback.Get(),UINT64(batch.slice)*StatsCapacity*sizeof(D3D12_QUERY_DATA_PIPELINE_STATISTICS));
    batch.closed=true;
}
void Reset(ID3D12GraphicsCommandList* list) {
    if(t_viewportList==list)t_viewportList=nullptr;
    if(!active.load(std::memory_order_relaxed))return;
    std::lock_guard lock(mutex);auto it=lists.find(list);if(it==lists.end())return;
    if(!it->second->claimed)++discarded;
    lists.erase(it);
}
Submission BeforeSubmit(UINT count,ID3D12CommandList* const* submitted) {
    Submission result;if(!active.load(std::memory_order_relaxed))return result;
    std::lock_guard lock(mutex);
    for(UINT i=0;i<count;++i) {
        auto it=lists.find(static_cast<ID3D12GraphicsCommandList*>(submitted[i]));if(it==lists.end())continue;
        auto& batch=*it->second;
        if(batch.claimed){batch.invalid=true;continue;}
        if(!batch.closed || batch.invalid){++discarded;lists.erase(it);continue;}
        batch.claimed=true;result.push_back(it->second);
    }
    return result;
}
void Submitted(ID3D12CommandQueue* queue,Submission values) {
    if(values.empty())return;
    ComPtr<ID3D12Device> device;ComPtr<ID3D12Fence> fence;uint64_t frequency{};
    const bool ok=SUCCEEDED(queue->GetDevice(IID_PPV_ARGS(&device))) &&
        SUCCEEDED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence))) &&
        SUCCEEDED(queue->GetTimestampFrequency(&frequency)) && frequency && SUCCEEDED(queue->Signal(fence.Get(),1));
    std::lock_guard lock(mutex);
    for(auto& batch:values) {
        batch->frequency=frequency;batch->fence=fence;batch->value=1;
        if(!ok){batch->invalid=true;batch->fence.Reset();}
        pending.push_back(std::move(batch));
    }
}
void Poll() {
    if(!active.load(std::memory_order_relaxed) && !CyberpunkVR_SceneGpuRequest.load(std::memory_order_relaxed) &&
       !CyberpunkVR_SceneGpuFrameRequest.load(std::memory_order_relaxed))return;
    std::lock_guard lock(mutex);
    if(!active.load() && pending.empty()) {
        const auto frames=std::min(CyberpunkVR_SceneGpuFrameRequest.exchange(0),32u);
        frameMode=frames>0;
        remaining=frameMode ? PoolLists : std::min(CyberpunkVR_SceneGpuRequest.exchange(0),256u);
        if(!remaining)return;
        if(frameMode){startSerial=Serial();endSerial=startSerial+frames;}
        completed=discarded=0;exhausted=false;statistics.clear();lists.clear();pool.reset();
        statsMode=CyberpunkVR_SceneGpuPipelineStats.load(std::memory_order_relaxed)!=0;
        occMode=CyberpunkVR_SceneGpuDrawOcclusion.load(std::memory_order_relaxed)!=0;occCounts={};active=true;
        deadline=std::chrono::steady_clock::now()+std::chrono::seconds(frameMode?5:3);
    }
    // Frame mode stops taking lists once the last frame asked for has been presented.
    if(frameMode && Serial()>endSerial)remaining=0;
    if(std::chrono::steady_clock::now()>deadline) {
        remaining=0;
        for(auto it=lists.begin();it!=lists.end();) {
            if(!it->second->claimed){++discarded;it=lists.erase(it);}else ++it;
        }
    }
    for(auto it=pending.begin();it!=pending.end();) {
        auto& batch=**it;
        if(batch.fence && batch.fence->GetCompletedValue()<batch.value){++it;continue;}
        if(!batch.invalid && batch.frequency) {
            void* data{};const size_t offset=size_t(batch.slice)*Capacity*16;
            D3D12_RANGE range{offset,offset+batch.ranges.size()*16};
            if(SUCCEEDED(batch.pool->readback->Map(0,&range,&data))) {
                const auto* stamps=reinterpret_cast<const uint64_t*>(static_cast<const uint8_t*>(data)+offset);
                for(size_t i=0;i<batch.ranges.size();++i) {
                    if(stamps[2*i+1]<stamps[2*i]){batch.invalid=true;break;}
                }
                std::set<Key> seen;
                if(!batch.invalid)for(size_t i=0;i<batch.ranges.size();++i) {
                    auto& stat=statistics[batch.ranges[i].key];++stat.ranges;stat.commands+=batch.ranges[i].commands;
                    if(seen.insert(batch.ranges[i].key).second)++stat.lists;
                    const auto ms=double(stamps[2*i+1]-stamps[2*i])*1000.0/double(batch.frequency);
                    stat.sumMs+=ms;stat.maxMs=std::max(stat.maxMs,ms);
                }
                D3D12_RANGE none{};batch.pool->readback->Unmap(0,&none);
            } else batch.invalid=true;
            if(!batch.invalid && !batch.drawKind.empty()) {
                const size_t occOffset=size_t(batch.slice)*DrawCapacity*8;
                D3D12_RANGE occRange{occOffset,occOffset+batch.drawKind.size()*8};
                if(SUCCEEDED(batch.pool->occReadback->Map(0,&occRange,&data))) {
                    const auto* samples=reinterpret_cast<const uint64_t*>(static_cast<const uint8_t*>(data)+occOffset);
                    for(size_t d=0;d<batch.drawKind.size();++d) {
                        auto& c=occCounts[batch.drawKind[d]];
                        const uint64_t n=samples[d];
                        const unsigned bucket=n==0?1:n<=64?2:n<=4096?3:4;
                        ++c[0];++c[bucket];
                        if(batch.drawRange[d]<batch.ranges.size()) {
                            auto& g=statistics[batch.ranges[batch.drawRange[d]].key].occ;++g[0];++g[bucket];
                        }
                    }
                    D3D12_RANGE none{};batch.pool->occReadback->Unmap(0,&none);
                }
            }
            if(!batch.invalid && batch.statsUsed) {
                constexpr size_t bytes=sizeof(D3D12_QUERY_DATA_PIPELINE_STATISTICS);
                const size_t statsOffset=size_t(batch.slice)*StatsCapacity*bytes;
                D3D12_RANGE statsRange{statsOffset,statsOffset+batch.statsUsed*bytes};
                if(SUCCEEDED(batch.pool->statsReadback->Map(0,&statsRange,&data))) {
                    const auto* values=reinterpret_cast<const uint64_t*>(static_cast<const uint8_t*>(data)+statsOffset);
                    for(const auto& r:batch.ranges) {
                        if(r.stat<0)continue;
                        auto& stat=statistics[r.key];stat.hasStats=true;
                        for(unsigned k=0;k<StatsCount;++k)stat.stats[k]+=values[size_t(r.stat)*StatsCount+k];
                    }
                    D3D12_RANGE none{};batch.pool->statsReadback->Unmap(0,&none);
                }
            }
        }
        if(batch.invalid)++discarded;else ++completed;
        for(auto mapIt=lists.begin();mapIt!=lists.end();) {
            if(mapIt->second==*it)mapIt=lists.erase(mapIt);else ++mapIt;
        }
        it=pending.erase(it);
    }
    if(!remaining && pending.empty() && lists.empty()){active=false;pool.reset();}
    Publish();
}
}
