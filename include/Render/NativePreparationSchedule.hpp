#pragma once
#include "Render/NativeGraphSnapshot.hpp"
#include <algorithm>
#include <memory>
#include <set>
#include <unordered_map>

namespace cvr::stereo::preparation_schedule {
using graph_snapshot::Field;
using Link=std::pair<uint32_t,uint32_t>;
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/preparation_schedule.rs (tools/single_pass_tests/preparation_schedule_test runs its
// cases against them in its Rust variant). The plan's members and the job graph's buffers stay here.
namespace detail {
struct RustIds {const uint32_t* data;size_t count;};
struct RustPlan {const std::array<uint32_t,2>* cpu;size_t cpuCount;RustIds ids[4];uint32_t failure;};
struct RustHeld;
static_assert(sizeof(RustPlan)==88,"mirrored by preparation_schedule.rs");
extern "C" {
RustHeld* cvr_ps_build(const graph_snapshot::Node*,size_t,const graph_snapshot::Edge*,size_t,uint32_t,uint64_t,RustPlan*);
void cvr_ps_free(RustHeld*);
bool cvr_ps_reach(const std::array<uint32_t,2>*,size_t,uint32_t,uint32_t);
bool cvr_ps_independent(const std::array<uint64_t,4>*);
uint32_t cvr_ps_job_records(const graph_snapshot::Node*,size_t,const std::array<uint32_t,2>*,size_t,std::array<uint8_t,48>*,
    std::array<uint32_t,6>*,uint32_t*);
void cvr_ps_job_header(std::array<uint8_t,0x70>*,uint64_t,uint32_t,uint64_t,uint64_t);
}
inline std::vector<std::array<uint32_t,2>> Flat(const std::set<Link>& cpu){
    std::vector<std::array<uint32_t,2>> flat;flat.reserve(cpu.size());for(const auto& [a,b]:cpu)flat.push_back({a,b});return flat;
}
inline std::vector<uint32_t> Ids(const RustIds& ids){return std::vector<uint32_t>(ids.data,ids.data+ids.count);}
}
#endif
struct Plan {
    std::set<Link> cpu;
    std::vector<uint32_t> early,ready,barriers,frontier;
    uint32_t failure{};
#if CVR_RUST_VRIK
    bool Reach(uint32_t from,uint32_t to)const{const auto flat=detail::Flat(cpu);return detail::cvr_ps_reach(flat.data(),flat.size(),from,to);}
    bool Build(const graph_snapshot::Snapshot& graph,uint64_t base){
        *this={};detail::RustPlan out{};
        const auto held=detail::cvr_ps_build(graph.nodes.data(),graph.nodes.size(),graph.edges.data(),graph.edges.size(),graph.failure,
            base,&out);
        struct Release{detail::RustHeld* held;~Release(){detail::cvr_ps_free(held);}}release{held};
        for(size_t i=0;i<out.cpuCount;++i)cpu.emplace_hint(cpu.end(),out.cpu[i][0],out.cpu[i][1]);
        early=detail::Ids(out.ids[0]);ready=detail::Ids(out.ids[1]);barriers=detail::Ids(out.ids[2]);frontier=detail::Ids(out.ids[3]);
        failure=out.failure;return !failure;
    }
#else
    bool Reach(uint32_t from,uint32_t to)const{
        std::set<uint32_t> seen{from};std::vector<uint32_t> pending{from};
        for(size_t i=0;i<pending.size();++i){if(pending[i]==to)return true;
            for(auto edge=cpu.lower_bound({pending[i],0});edge!=cpu.end() && edge->first==pending[i];++edge)
                if(seen.insert(edge->second).second)pending.push_back(edge->second);}
        return false;
    }
    bool Build(const graph_snapshot::Snapshot& graph,uint64_t base){
        *this={};failure=1;
        if(graph.failure || graph.nodes.empty() || graph.nodes.size()>255 || graph.edges.size()>65536)return false;
        std::unordered_map<uint32_t,const graph_snapshot::Node*> nodes;
        std::set<uint32_t> ordinals;
        std::array<uint32_t,2> prepare{},scene{};std::array<uint32_t,2> prepares{},scenes{},culls{},particles{};
        for(const auto& n:graph.nodes){const auto id=n.handle&0xDFFFFFFFu,view=Field<uint32_t>(n.bytes,24),ordinal=Field<uint32_t>(n.bytes,0);
            if(view>1 || ordinal!=n.order || !nodes.emplace(id,&n).second || !ordinals.insert(ordinal).second)return false;
            const auto work=n.work>=base?n.work-base:0;
            if(work==0xC0F204){prepare[view]=id;++prepares[view];}
            if(work==0xBF436C){++culls[view];if(Field<uint32_t>(n.definition,24)==17){scene[view]=id;++scenes[view];}}
            if(work==0xD5BAC4)++particles[view];
        }
        if(prepares!=std::array<uint32_t,2>{1,1} || scenes!=std::array<uint32_t,2>{1,1} ||
            culls!=std::array<uint32_t,2>{3,3} || particles!=std::array<uint32_t,2>{4,4})return false;
        for(const auto& edge:graph.edges){const auto phase=Field<uint32_t>(edge.bytes,4);
            const auto from=Field<uint32_t>(edge.bytes,8)&0xDFFFFFFFu,to=Field<uint32_t>(edge.bytes,12)&0xDFFFFFFFu;
            if(phase>1 || !nodes.contains(from) || !nodes.contains(to) || from==to)return false;
            if(!phase && !cpu.emplace(from,to).second)return false;
        }
        failure=2;
        std::vector<uint32_t> incoming;
        for(const auto& [a,b]:cpu)if(b==scene[1])incoming.push_back(a);
        if(incoming.size()!=1)return false;
        const auto bridge=incoming[0];const auto* node=nodes.at(bridge);
        if(node->work || Field<uint32_t>(node->bytes,24)!=1 || !cpu.contains({prepare[1],bridge}))return false;
        for(const auto& [a,b]:cpu)if(b==bridge && a!=prepare[1])return false;
        early={prepare[1],bridge,scene[1]};
        const auto isEarly=[&](uint32_t id){return std::find(early.begin(),early.end(),id)!=early.end();};
        for(const auto& [a,b]:cpu){
            if(!isEarly(a) && isEarly(b) && b!=prepare[1])return false;
            if(isEarly(a) && !isEarly(b))frontier.push_back(b);
            if(b==prepare[1] && Reach(prepare[0],a))barriers.push_back(a);
        }
        std::sort(frontier.begin(),frontier.end());frontier.erase(std::unique(frontier.begin(),frontier.end()),frontier.end());
        if(frontier.empty() || barriers.empty())return false;
        ready.push_back(prepare[0]);
        for(const auto& [id,n]:nodes){if(Field<uint32_t>(n->bytes,24)!=0)continue;
            const auto work=n->work>=base?n->work-base:0;
            if(work==0xBF436C || work==0xD5BAC4){if(!Reach(prepare[0],id))return false;ready.push_back(id);}
        }
        const auto original=cpu;
        for(const auto barrier:barriers){
            cpu.erase({barrier,prepare[1]});
            // Only the collector/query/materializer move. Every outgoing path
            // rejoins the original MAIN barriers before the remaining eye work.
            for(const auto target:frontier)cpu.emplace(barrier,target);
            cpu.emplace(scene[1],barrier);
        }
        for(const auto source:ready)cpu.emplace(source,prepare[1]);
        failure=3;
        std::unordered_map<uint32_t,uint32_t> degree;
        for(const auto& [id,n]:nodes)degree[id]=0;
        for(const auto& [a,b]:cpu)++degree[b];
        std::vector<uint32_t> pending;
        for(const auto& [id,d]:degree)if(!d)pending.push_back(id);
        for(size_t i=0;i<pending.size();++i)
            for(auto edge=cpu.lower_bound({pending[i],0});edge!=cpu.end() && edge->first==pending[i];++edge)
                if(!--degree[edge->second])pending.push_back(edge->second);
        if(pending.size()!=nodes.size())return false;
        failure=4;
        for(const auto& [a,b]:original)if(!isEarly(a) && !isEarly(b) && !cpu.contains({a,b}))return false;
        for(const auto source:barriers)for(const auto target:frontier)if(!Reach(source,target))return false;
        failure=0;return true;
    }
#endif
};

// 0x8EA1C8 consumes these fields synchronously to create native job-group
// dependencies. 0x32D60C still receives the untouched original graph. Do not
// use this partial shadow with graph mutation, sorting or execution APIs.
struct JobGraph {
    alignas(8) std::array<uint8_t,0x70> header{};
    std::vector<std::array<uint8_t,48>> nodes;
    std::vector<std::array<uint32_t,6>> edges;
    std::vector<uint32_t> edgeOrder;
    template<class T>void Put(size_t offset,T value){std::memcpy(header.data()+offset,&value,sizeof(value));}
#if CVR_RUST_VRIK
    void Build(const graph_snapshot::Snapshot& source,const Plan& plan){
        const auto cpu=detail::Flat(plan.cpu);
        nodes.resize(detail::cvr_ps_job_records(source.nodes.data(),source.nodes.size(),nullptr,0,nullptr,nullptr,nullptr));
        edges.resize(cpu.size());edgeOrder.resize(cpu.size());
        detail::cvr_ps_job_records(source.nodes.data(),source.nodes.size(),cpu.data(),cpu.size(),nodes.data(),edges.data(),edgeOrder.data());
        detail::cvr_ps_job_header(&header,reinterpret_cast<uintptr_t>(nodes.data()),uint32_t(edges.size()),
            reinterpret_cast<uintptr_t>(edges.data()),reinterpret_cast<uintptr_t>(edgeOrder.data()));
    }
#else
    void Build(const graph_snapshot::Snapshot& source,const Plan& plan){
        uint32_t size{};for(const auto& n:source.nodes)size=std::max(size,(n.handle&0xDFFFFFFFu)+1);
        nodes.resize(size);for(const auto& n:source.nodes)nodes[n.handle&0xDFFFFFFFu]=n.bytes;
        edges.reserve(plan.cpu.size());edgeOrder.reserve(plan.cpu.size());
        for(const auto& [from,to]:plan.cpu){const auto index=uint32_t(edges.size());
            edges.push_back({index,0,from|0x20000000u,to|0x20000000u,0x80000000u,0x80000000u});edgeOrder.push_back(index);}
        Put(0x18,reinterpret_cast<uintptr_t>(nodes.data()));Put(0x40,uint32_t(edges.size()));
        Put(0x48,reinterpret_cast<uintptr_t>(edges.data()));Put(0x58,reinterpret_cast<uintptr_t>(edgeOrder.data()));
    }
#endif
};
struct PreparedPlan {
    PreparedPlan()=default;
    PreparedPlan(const PreparedPlan&)=delete;
    PreparedPlan& operator=(const PreparedPlan&)=delete;
    graph_snapshot::Snapshot source;
    uint64_t base{};
    Plan plan;
    JobGraph jobs;
    std::vector<std::array<uint32_t,2>> edges;
    bool Matches(const graph_snapshot::Snapshot& value,uint64_t image)const {
        return !value.failure && base==image && source.nodes==value.nodes && source.edges==value.edges;
    }
};
// Callers serialize acquisition. The returned immutable owner remains alive
// through native dependency creation even if another graph replaces the cache.
class PlanCache {
public:
    uint32_t failure{};
    std::shared_ptr<const PreparedPlan> Acquire(const graph_snapshot::Snapshot& source,uint64_t base,bool reuse,bool& hit){
        failure=0;hit=false;
        if(reuse)for(size_t i=0;i<recent.size();++i)if(recent[i] && recent[i]->Matches(source,base)){
            hit=true;auto result=recent[i];
            std::move_backward(recent.begin(),recent.begin()+i,recent.begin()+i+1);recent[0]=result;return result;
        }
        auto next=std::make_shared<PreparedPlan>();next->source=source;next->base=base;
        if(!next->plan.Build(source,base)){failure=next->plan.failure;return {};}
        next->jobs.Build(source,next->plan);
        for(const auto& [a,b]:next->plan.cpu)next->edges.push_back({a,b});
        if(!reuse)recent={};
        std::move_backward(recent.begin(),recent.end()-1,recent.end());recent[0]=next;return next;
    }
private:
    std::array<std::shared_ptr<const PreparedPlan>,4> recent;
};
#if CVR_RUST_VRIK
inline bool Independent(const std::array<uint64_t,4>& ranges){return detail::cvr_ps_independent(&ranges);}
#else
inline bool Independent(const std::array<uint64_t,4>& ranges){
    // 2.5: graph container 0xE0500 (sub_140423180 add r8,0E0500h), view extent 0x3130 (2.31: 0xC5C40 / 0x22C0).
    constexpr std::array<uint64_t,4> sizes{0xE0500,0x3130,0xE0500,0x3130};
    for(size_t i=0;i<4;++i){if(!ranges[i] || ranges[i]>UINT64_MAX-sizes[i])return false;
        for(size_t j=0;j<i;++j)if(ranges[i]<ranges[j]+sizes[j] && ranges[j]<ranges[i]+sizes[i])return false;}
    return true;
}
#endif
}
