#include "Render/PairedPassFrontier.hpp"
#include <cstdio>
#include <fstream>
#include <source_location>
#include <stdexcept>
#include <string>

using namespace cvr::stereo;
using namespace paired_frontier;
static constexpr uint64_t Base=0x140000000;
static void Require(bool value,std::source_location where=std::source_location::current()){
    if(!value)throw std::runtime_error("Paired-pass frontier assertion failed at line "+std::to_string(where.line()));
}
template<class T,size_t N>static void Put(std::array<uint8_t,N>& bytes,size_t offset,T value){std::memcpy(bytes.data()+offset,&value,sizeof(value));}
struct Fixture {
    graph_snapshot::Snapshot graph;
    std::array<Roles,2> roles;
    std::vector<uint32_t> order;
    uint32_t Node(uint32_t eye,uint32_t work,uint64_t tag=0){
        const auto id=uint32_t(graph.nodes.size());graph_snapshot::Node node;node.handle=id;node.order=id;
        Put(node.bytes,0,id);Put(node.bytes,8,uint32_t(4));Put(node.bytes,16,uint64_t(0x100000+id*64));Put(node.bytes,24,eye);
        Put(node.bytes,28,uint32_t(id|(id<<8)));node.vtable=Base+0x2800000+id*16;node.work=Base+work;
        Put(node.definition,0,node.vtable);Put(node.definition,24,tag);graph.nodes.push_back(node);order.push_back(id);return id;
    }
    uint32_t Named(uint32_t eye,std::string_view name){return Node(eye,0x332D5C,Name(name));}
    void Edge(uint32_t phase,uint32_t source,uint32_t target){
        graph_snapshot::Edge edge;edge.handle=edge.order=uint32_t(graph.edges.size());
        Put(edge.bytes,0,edge.order);Put(edge.bytes,4,phase);Put(edge.bytes,8,source|0x20000000u);Put(edge.bytes,12,target|0x20000000u);
        Put(edge.bytes,16,0x80000000u);Put(edge.bytes,20,0x80000000u);graph.edges.push_back(edge);
    }
    Fixture(){
        Named(0,"StartRender");
        for(uint32_t eye=0;eye<2;++eye){auto& role=roles[eye];role.collector=Node(eye,0xC0F204);
            for(const uint32_t kind:{17u,2u,4u})Node(eye,0xBF436C,kind);
            for(uint32_t particle=0;particle<4;++particle)Node(eye,0xD5BAC4,uint64_t(4)<<32|particle);
            Node(eye,0x77596C);Named(eye,"RenderPrep");Named(eye,"PrepareGloballyAccessibleSystems");Node(eye,0x655FAC,1);
            Named(eye,"AsyncWater");role.depth=Named(eye,"DepthPrepass");Named(eye,"GBufferEarly_Velocity_Weapon");Node(eye,0xCB1E88);
            for(uint32_t solid=0;solid<4;++solid)role.solid.push_back(Named(eye,"GBuffer_Solid"));
            role.solidReady=Node(eye,0x655FAC,2);role.discard=Named(eye,"GBuffer_Discard");
            role.afterDiscard=role.postGBuffer=Named(eye,"PostGBufferOverrides");role.lighting=Named(eye,"Lighting");role.final2D=Named(eye,"RenderFinal2D");
        }
        for(uint32_t phase=0;phase<2;++phase)for(size_t i=1;i<order.size();++i)Edge(phase,order[i-1],order[i]);
    }
};
static void Positive(const graph_snapshot::Snapshot& source,uint64_t base,const std::array<Roles,2>* expected=nullptr,Boundary boundary=Boundary::Solid){
    const auto original=source;Plan plan;
    Require(plan.BuildCandidate(source,base,boundary)==Result::CandidateNeedsCpuAndResourceCompile && plan.Rejection()==Failure::None && !plan.ExecutionReady());
    Require(plan.SelectedBoundary()==boundary);
    Require(source.nodes==original.nodes && source.edges==original.edges && plan.Matches(source,base) && !plan.Matches(source,base+1));
    Require(plan.Source() && plan.Source()->nodes==source.nodes && plan.Source()->edges==source.edges && plan.SourceFingerprint());
    const auto& roles=plan.EyeRoles();const auto& proof=plan.Proofs();
    Require(proof.originalCpuAcyclic && proof.originalOrderAcyclic && proof.candidateOrderAcyclic && proof.cpuUnchanged &&
        proof.unrelatedOrderEdgesPreserved && proof.bothProducersBeforeMainDiscard && proof.mainGeometryBeforeLighting && proof.peerGeometryBeforeLighting &&
        !proof.cpuProducerClosureProved && !proof.resourceOrderCompiled && !proof.parentPassMappingProved);
    Require(plan.Removed()[0].empty() && plan.Added()[0].empty() && plan.Original()[0]==plan.Proposed()[0]);
    const auto tail=boundary==Boundary::Discard?roles[1].discard:roles[1].solidReady;
    const auto continuation=boundary==Boundary::Discard?roles[1].afterDiscard:roles[1].discard;
    Require(plan.EarlyTail()==tail);
    Require(plan.Removed()[1]==Edges{{roles[0].solidReady,roles[0].discard},{roles[0].final2D,roles[1].collector},{tail,continuation}});
    Require(plan.Added()[1]==Edges{{roles[0].solidReady,roles[1].collector},{tail,roles[0].discard},{roles[0].final2D,continuation}});
    if(boundary==Boundary::Discard)Require(plan.Proposed()[1].contains({roles[1].solidReady,roles[1].discard}) &&
        !plan.Proposed()[1].contains({roles[1].discard,roles[1].afterDiscard}) &&
        detail::Path(plan.Proposed()[1],roles[1].discard,roles[0].discard)==std::vector<uint32_t>{roles[1].discard,roles[0].discard} &&
        !detail::Path(plan.Proposed()[1],roles[0].final2D,roles[1].afterDiscard).empty());
    auto reconstructed=plan.Original()[1];for(const auto edge:plan.Removed()[1])Require(reconstructed.erase(edge)==1);
    for(const auto edge:plan.Added()[1])Require(reconstructed.insert(edge).second);Require(reconstructed==plan.Proposed()[1]);
    Require(!plan.CpuBlockerPath().empty() && !plan.OrderBlockerPath().empty() && !plan.PeerCpuClosure().empty() && !plan.CpuIncoming().empty());
    Require(plan.CpuBlockerPath().front()==roles[0].discard && plan.CpuBlockerPath().back()==tail);
    Require(plan.OrderBlockerPath().front()==roles[0].discard && plan.OrderBlockerPath().back()==tail);
    Require(std::find(plan.PeerCpuClosure().begin(),plan.PeerCpuClosure().end(),tail)!=plan.PeerCpuClosure().end());
    if(expected)for(size_t eye=0;eye<2;++eye){const auto& a=roles[eye];const auto& b=(*expected)[eye];
        Require(a.collector==b.collector && a.depth==b.depth && a.solidReady==b.solidReady && a.discard==b.discard &&
            a.afterDiscard==b.afterDiscard && a.postGBuffer==b.postGBuffer && a.lighting==b.lighting && a.final2D==b.final2D && a.solid==b.solid && a.prefix.size()==21);}
    auto mutated=source;mutated.nodes[0].bytes[29]^=1;Require(!plan.Matches(mutated,base));
    mutated=source;mutated.nodes[0].definition[16]^=1;Require(!plan.Matches(mutated,base));
    mutated=source;mutated.edges[0].bytes[16]^=1;Require(!plan.Matches(mutated,base));
    mutated=source;mutated.failure=1;Require(!plan.Matches(mutated,base));
    auto held=plan.Source();const auto heldHash=plan.SourceFingerprint();plan=Plan{};
    Require(held && held->nodes==source.nodes && heldHash==detail::Fingerprint(*held,base));
    Require(plan.BuildCandidate(source,base,boundary)==Result::CandidateNeedsCpuAndResourceCompile);
    const auto* owned=plan.Source().get();
    Require(plan.BuildCandidate(*owned,base,boundary)==Result::CandidateNeedsCpuAndResourceCompile && plan.Matches(source,base) && plan.SelectedBoundary()==boundary);
}
static void Negative(const graph_snapshot::Snapshot& graph,Failure reason,Boundary boundary=Boundary::Solid,
                     std::source_location where=std::source_location::current()){
    Plan plan;const auto result=plan.BuildCandidate(graph,Base,boundary);
    if(result!=Result::Rejected || plan.Rejection()!=reason)
        throw std::runtime_error("Paired-pass rejection at line "+std::to_string(where.line())+": expected "+
            std::to_string(uint32_t(reason))+", got "+std::to_string(uint32_t(plan.Rejection())));
    Require(!plan.ExecutionReady() && !plan.Matches(graph,Base),where);
}
template<class T>static std::vector<T> File(const char* path){
    std::ifstream stream(path,std::ios::binary|std::ios::ate);Require(bool(stream));const auto bytes=stream.tellg();
    Require(bytes>=0 && uint64_t(bytes)%sizeof(T)==0 && uint64_t(bytes)<=64*1024*1024);
    std::vector<T> result(size_t(bytes)/sizeof(T));stream.seekg(0);if(bytes)stream.read(reinterpret_cast<char*>(result.data()),bytes);Require(bool(stream));return result;
}
int main(int argc,char** argv)try{
    Require(argc==1 || argc==4);Require(Name("Lighting")==0xDA843B18B409BFA9ull && Name("GBuffer_Solid")==0xCA1FC9259390192Cull);
    Fixture fixture;Positive(fixture.graph,Base,&fixture.roles);Positive(fixture.graph,Base,&fixture.roles,Boundary::Discard);
    Negative(fixture.graph,Failure::Source,static_cast<Boundary>(2));
    auto changed=fixture.graph;changed.failure=1;Negative(changed,Failure::Source);
    changed=fixture.graph;changed.nodes.clear();Negative(changed,Failure::Source);
    changed=fixture.graph;changed.nodes[1].handle=changed.nodes[0].handle;Negative(changed,Failure::Nodes);
    changed=fixture.graph;++changed.nodes[1].order;Negative(changed,Failure::Nodes);
    changed=fixture.graph;Put(changed.nodes[1].bytes,24,uint32_t(2));Negative(changed,Failure::Nodes);
    changed=fixture.graph;changed.nodes[1].definition[0]^=1;Negative(changed,Failure::Nodes);
    changed=fixture.graph;Put(changed.edges[0].bytes,4,uint32_t(2));Negative(changed,Failure::Edges);
    changed=fixture.graph;Put(changed.edges[0].bytes,12,uint32_t(0x2000ffff));Negative(changed,Failure::Edges);
    changed=fixture.graph;changed.edges[1].handle=changed.edges[0].handle;Negative(changed,Failure::Edges);
    changed=fixture.graph;for(size_t i=8;i<16;++i)changed.edges[1].bytes[i]=changed.edges[0].bytes[i];Negative(changed,Failure::Edges);
    Fixture cyclic;cyclic.Edge(0,cyclic.roles[1].final2D,cyclic.roles[0].discard);Negative(cyclic.graph,Failure::CpuCycle);
    cyclic=Fixture{};cyclic.Edge(1,cyclic.roles[1].final2D,cyclic.roles[0].discard);Negative(cyclic.graph,Failure::OrderCycle);
    changed=fixture.graph;Put(changed.nodes[fixture.roles[0].lighting].definition,24,Name("UnknownLighting"));Negative(changed,Failure::Roles);
    changed=fixture.graph;Put(changed.nodes[fixture.roles[0].postGBuffer].definition,24,Name("Lighting"));Negative(changed,Failure::Roles);
    changed=fixture.graph;changed.nodes[fixture.roles[1].collector+8].work=Base+0x123456;Negative(changed,Failure::Prefix);
    Fixture forked;forked.Edge(1,forked.roles[1].collector,forked.roles[1].depth);Negative(forked.graph,Failure::Prefix);
    changed=fixture.graph;for(auto& edge:changed.edges)if(Field<uint32_t>(edge.bytes,4)==1 &&
        (Field<uint32_t>(edge.bytes,8)&0xDFFFFFFFu)==fixture.roles[0].final2D && (Field<uint32_t>(edge.bytes,12)&0xDFFFFFFFu)==fixture.roles[1].collector)
        Put(edge.bytes,8,fixture.roles[0].lighting|0x20000000u);
    Negative(changed,Failure::OrderBoundary);
    Fixture discardFork;discardFork.Edge(1,discardFork.roles[1].discard,discardFork.roles[1].lighting);
    Negative(discardFork.graph,Failure::OrderBoundary,Boundary::Discard);
    Fixture suffixFork;suffixFork.Edge(1,suffixFork.roles[1].postGBuffer,suffixFork.roles[1].final2D);
    Negative(suffixFork.graph,Failure::OrderBoundary,Boundary::Discard);
    Fixture foreignIncoming;foreignIncoming.Edge(1,0,foreignIncoming.roles[1].afterDiscard);
    Negative(foreignIncoming.graph,Failure::OrderBoundary,Boundary::Discard);
    Fixture discardIncoming;discardIncoming.Edge(1,discardIncoming.roles[0].discard,discardIncoming.roles[1].discard);
    Negative(discardIncoming.graph,Failure::Roles,Boundary::Discard);
    changed=fixture.graph;for(auto& edge:changed.edges)if(Field<uint32_t>(edge.bytes,4)==1 &&
        (Field<uint32_t>(edge.bytes,8)&0xDFFFFFFFu)==fixture.roles[1].discard)
        Put(edge.bytes,12,fixture.roles[1].lighting|0x20000000u);
    Negative(changed,Failure::OrderBoundary,Boundary::Discard);

    // Handles are independent of publication ordinals and can be tagged.
    changed=fixture.graph;std::map<uint32_t,uint32_t> remap;
    for(const auto& node:changed.nodes)remap[node.handle]=1000+(uint32_t(changed.nodes.size())-1-node.handle)*7;
    auto remappedRoles=fixture.roles;
    for(auto& node:changed.nodes)node.handle=remap.at(node.handle)|0x20000000u;
    for(auto& edge:changed.edges){for(size_t offset:{size_t(8),size_t(12)})Put(edge.bytes,offset,remap.at(Field<uint32_t>(edge.bytes,offset)&0xDFFFFFFFu)|0x20000000u);
        edge.handle=(edge.handle+2000)|0x40000000u;}
    for(auto& role:remappedRoles){for(auto* field:{&role.collector,&role.depth,&role.solidReady,&role.discard,&role.afterDiscard,&role.postGBuffer,&role.lighting,&role.final2D})*field=remap.at(*field);
        for(auto& solid:role.solid)solid=remap.at(solid);std::sort(role.solid.begin(),role.solid.end());}
    Positive(changed,Base,&remappedRoles);
    Positive(changed,Base,&remappedRoles,Boundary::Discard);
    constexpr uint64_t relocation=0x100000000;
    for(auto& node:changed.nodes){node.work+=relocation;node.vtable+=relocation;Put(node.definition,0,node.vtable);}
    Positive(changed,Base+relocation,&remappedRoles);
    Positive(changed,Base+relocation,&remappedRoles,Boundary::Discard);
    if(argc==4){graph_snapshot::Snapshot saved;saved.nodes=File<graph_snapshot::Node>(argv[1]);saved.edges=File<graph_snapshot::Edge>(argv[2]);
        const auto base=std::stoull(argv[3],nullptr,0);Positive(saved,base);Positive(saved,base,nullptr,Boundary::Discard);Plan plan;plan.BuildCandidate(saved,base,Boundary::Discard);
        const auto& main=plan.EyeRoles()[0];const auto& peer=plan.EyeRoles()[1];
        Require(main.collector==5 && main.solidReady==25 && main.discard==26 && main.postGBuffer==32 && main.lighting==45 && main.final2D==57);
        Require(peer.collector==88 && peer.solidReady==108 && peer.discard==109 && peer.afterDiscard==110 && peer.postGBuffer==115 && peer.lighting==128 && peer.final2D==140);
        Require(saved.nodes.size()==155 && plan.Original()[0].size()==809 && plan.Original()[1].size()==119);
        Require(plan.EarlyTail()==109 && plan.Proposed()[1].size()==119 && plan.Proposed()[1].contains({108,109}) &&
            plan.Removed()[1]==Edges{{25,26},{57,88},{109,110}} && plan.Added()[1]==Edges{{25,88},{109,26},{57,110}});
        std::printf("PASS saved graph: role-derived solid/discard topology, source %016llx; native context/lifetime/physical-alias proofs remain pending\n",
            static_cast<unsigned long long>(plan.SourceFingerprint()));
    }
    std::puts("PASS paired-pass frontier: solid/discard boundaries, dual DAGs, immutable source, remapped handles and ambiguous suffix rejection");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
