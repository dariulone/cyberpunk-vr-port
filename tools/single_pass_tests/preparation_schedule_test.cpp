#include "Render/NativePreparationSchedule.hpp"
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <string>
#include <chrono>

using namespace cvr::stereo;
void Check(bool value){if(!value)throw std::runtime_error("preparation schedule check failed");}
template<class T>void Put(std::array<uint8_t,48>& bytes,size_t offset,T value){std::memcpy(bytes.data()+offset,&value,sizeof(value));}
struct Fixture {
    graph_snapshot::Snapshot graph;
    uint64_t base=0x140000000;
    uint32_t Node(uint32_t view,uint32_t work,uint32_t flags=0){
        graph_snapshot::Node n;n.handle=uint32_t(graph.nodes.size())+10;n.order=uint32_t(graph.nodes.size());
        Put(n.bytes,0,n.order);Put(n.bytes,8,uint32_t(4));Put(n.bytes,24,view);Put(n.bytes,28,uint32_t(n.order*257));
        n.work=work?base+work:0;std::memcpy(n.definition.data()+24,&flags,4);graph.nodes.push_back(n);return n.handle;
    }
    void Edge(uint32_t a,uint32_t b,uint32_t phase=0){graph_snapshot::Edge edge;edge.handle=edge.order=uint32_t(graph.edges.size());
        std::array<uint32_t,6> data{edge.order,phase,a|0x20000000,b|0x20000000,0x80000000,0x80000000};
        std::memcpy(edge.bytes.data(),data.data(),24);graph.edges.push_back(edge);}
    uint32_t main{},eye{},scene{},render{},bridge{},tail{};
    Fixture(){
        main=Node(0,0xC0F204);std::vector<uint32_t> ready;
        for(auto flags:{17,2,4}){auto c=Node(0,0xBF436C,flags);Edge(main,c);ready.push_back(c);}
        for(int i=0;i<4;++i){auto p=Node(0,0xD5BAC4);Edge(ready[0],p);ready.push_back(p);}
        render=Node(0,0x332D5C);Edge(ready[0],render);
        eye=Node(1,0xC0F204);Edge(render,eye);Edge(render,eye,1);
        bridge=Node(1,0);Edge(eye,bridge);scene=Node(1,0xBF436C,17);Edge(bridge,scene);
        tail=Node(1,0);Edge(scene,tail);Edge(bridge,tail);
        for(auto flags:{2,4}){auto c=Node(1,0xBF436C,flags);Edge(eye,c);}
        for(int i=0;i<4;++i){auto p=Node(1,0xD5BAC4);Edge(tail,p);}
    }
};
template<class T>std::vector<T> Load(const char* path){std::ifstream file(path,std::ios::binary|std::ios::ate);Check(bool(file));
    const auto bytes=file.tellg();Check(bytes>0 && size_t(bytes)%sizeof(T)==0);std::vector<T> data(size_t(bytes)/sizeof(T));
    file.seekg(0);file.read(reinterpret_cast<char*>(data.data()),bytes);Check(bool(file));return data;}
int main(int argc,char** argv){try{
    Fixture f;preparation_schedule::Plan plan;
    const auto before=f.graph.nodes;const auto edges=f.graph.edges;
    Check(plan.Build(f.graph,f.base));Check(plan.Reach(f.scene,f.render));Check(!plan.Reach(f.render,f.eye));
    const auto fullScan=[&](uint32_t from,uint32_t to){
        std::set<uint32_t> seen{from};std::vector<uint32_t> pending{from};
        for(size_t i=0;i<pending.size();++i){if(pending[i]==to)return true;
            for(const auto& [a,b]:plan.cpu)if(a==pending[i] && seen.insert(b).second)pending.push_back(b);}
        return false;
    };
    for(const auto& a:f.graph.nodes)for(const auto& b:f.graph.nodes)Check(plan.Reach(a.handle,b.handle)==fullScan(a.handle,b.handle));
    Check(!plan.Reach(9999,f.main) && plan.Reach(9999,9999));
    Check(plan.Reach(f.render,f.tail));Check(plan.early.size()==3 && plan.ready.size()==8 && plan.frontier.size()==3);
    for(auto ready:plan.ready)Check(plan.Reach(ready,f.eye));
    for(size_t i=0;i<before.size();++i)Check(before[i].bytes==f.graph.nodes[i].bytes);
    for(size_t i=0;i<edges.size();++i)Check(edges[i].bytes==f.graph.edges[i].bytes);
    preparation_schedule::JobGraph shadow;shadow.Build(f.graph,plan);
    Check(graph_snapshot::Field<uint32_t>(shadow.header,0x40)==plan.cpu.size());
    for(const auto& edge:shadow.edges){Check(edge[1]==0);const auto a=edge[2]&0xDFFFFFFF,b=edge[3]&0xDFFFFFFF;
        Check(plan.cpu.contains({a,b}));Check(graph_snapshot::Field<uint32_t>(shadow.nodes[a],0)<f.graph.nodes.size());}
    Fixture cycle;cycle.Edge(cycle.render,cycle.main);Check(!plan.Build(cycle.graph,cycle.base) && plan.failure==3);
    Fixture unexpected;unexpected.Edge(unexpected.render,unexpected.scene);Check(!plan.Build(unexpected.graph,unexpected.base));
    Fixture unknown;unknown.Node(2,0);Check(!plan.Build(unknown.graph,unknown.base));
    Fixture ordinal;Put(ordinal.graph.nodes[0].bytes,0,uint32_t(3));Check(!plan.Build(ordinal.graph,ordinal.base));
    Fixture missing;missing.graph.nodes[2].work=0;Check(!plan.Build(missing.graph,missing.base));
    Check(preparation_schedule::Independent({0x100000,0x1E0500,0x200000,0x2E0500}));
    Check(!preparation_schedule::Independent({0x100000,0x1E0500,0x200000,0x1E0500}));
    Check(!preparation_schedule::Independent({0,1,2,3}));
    preparation_schedule::PlanCache cache;bool hit{};
    const auto first=cache.Acquire(f.graph,f.base,true,hit);Check(first && !hit && !cache.failure);
    Check(cache.Acquire(f.graph,f.base,true,hit)==first && hit);
    const auto storedNodes=first->jobs.nodes;const auto storedEdges=first->jobs.edges;
    for(unsigned field=0;field<8;++field){
        auto changed=f.graph;
        switch(field){
        case 0:changed.nodes[0].bytes[47]^=1;break;
        case 1:changed.nodes[0].vtable^=8;break;
        case 2:changed.nodes[8].work+=4;break;
        case 3:changed.nodes[0].definition[0]^=1;break;
        case 4:changed.edges[0].bytes[20]^=1;break;
        case 5:changed.edges[0].handle+=100;break;
        case 6:changed.edges[0].order+=100;break;
        case 7:changed.nodes[0].order+=1;break;
        }
        const auto result=cache.Acquire(changed,f.base,true,hit);
        Check(!hit && result!=first);
        Check(first->jobs.nodes==storedNodes && first->jobs.edges==storedEdges);
        Check(graph_snapshot::Field<uint64_t>(first->jobs.header,0x18)==reinterpret_cast<uintptr_t>(first->jobs.nodes.data()));
        Check(graph_snapshot::Field<uint64_t>(first->jobs.header,0x48)==reinterpret_cast<uintptr_t>(first->jobs.edges.data()));
        Check(cache.Acquire(f.graph,f.base,true,hit)!=nullptr);
    }
    const auto rebuilt=cache.Acquire(f.graph,f.base,false,hit);Check(rebuilt && !hit && rebuilt!=first);
    Check(cache.Acquire(f.graph,f.base,true,hit)==rebuilt && hit);
    preparation_schedule::PlanCache variants;std::array<std::shared_ptr<const preparation_schedule::PreparedPlan>,5> leases;
    for(unsigned i=0;i<4;++i){auto changed=f.graph;changed.nodes[0].bytes[47]=uint8_t(i);
        leases[i]=variants.Acquire(changed,f.base,true,hit);Check(leases[i] && !hit);}
    const auto evictedNodes=leases[1]->jobs.nodes;
    Check(variants.Acquire(f.graph,f.base,true,hit)==leases[0] && hit);
    auto fifth=f.graph;fifth.nodes[0].bytes[47]=4;leases[4]=variants.Acquire(fifth,f.base,true,hit);Check(leases[4] && !hit);
    Check(variants.Acquire(f.graph,f.base,true,hit)==leases[0] && hit);
    auto evicted=f.graph;evicted.nodes[0].bytes[47]=1;
    Check(variants.Acquire(evicted,f.base,true,hit)!=leases[1] && !hit);
    Check(leases[1]->jobs.nodes==evictedNodes && leases[1]->jobs.nodes[f.graph.nodes[0].handle][47]==1);
    Check(graph_snapshot::Field<uint64_t>(leases[1]->jobs.header,0x18)==reinterpret_cast<uintptr_t>(leases[1]->jobs.nodes.data()));
    auto failed=f.graph;failed.failure=5;Check(!cache.Acquire(failed,f.base,true,hit) && !hit && cache.failure);
    Check(!cache.Acquire(f.graph,f.base+1,true,hit) && !hit && cache.failure);
    Check(cache.Acquire(f.graph,f.base,true,hit)==rebuilt && hit);
    if(argc>=4){graph_snapshot::Snapshot actual;actual.nodes=Load<graph_snapshot::Node>(argv[1]);actual.edges=Load<graph_snapshot::Edge>(argv[2]);
        const auto image=std::stoull(argv[3],nullptr,0);const unsigned iterations=argc==5?std::stoul(argv[4]):1;
        Check(iterations>0 && iterations<=1000);
        const auto start=std::chrono::steady_clock::now();
        for(unsigned i=0;i<iterations;++i)Check(plan.Build(actual,image));
        const auto milliseconds=std::chrono::duration<double,std::milli>(std::chrono::steady_clock::now()-start).count();
        std::cout<<"captured plan: early="<<plan.early.size()<<" ready="<<plan.ready.size()<<" barriers="<<plan.barriers.size()<<" frontier="<<plan.frontier.size()<<" CPU edges="<<plan.cpu.size()<<'\n';
        std::cout<<"plan_build iterations="<<iterations<<" total_ms="<<milliseconds<<" mean_ms="<<milliseconds/iterations<<'\n';}
    std::cout<<"preparation schedule passed\n";return 0;
}catch(const std::exception& error){std::cerr<<error.what()<<'\n';return 1;}}
