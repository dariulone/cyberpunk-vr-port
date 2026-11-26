#include "Render/SceneGraphExpansion.hpp"
#include <cstdio>
#include <fstream>
#include <stdexcept>

using namespace cvr::stereo;
namespace {
constexpr uint64_t Base=0x140000000;
void Require(bool value){if(!value)throw std::runtime_error("Scene graph expansion assertion failed");}
template<class T,size_t N>void Put(std::array<uint8_t,N>& bytes,size_t offset,T value){std::memcpy(bytes.data()+offset,&value,sizeof(value));}
graph_snapshot::Snapshot Fixture(){
    graph_snapshot::Snapshot source;
    for(uint32_t i=0;i<6;++i){graph_snapshot::Node node;node.handle=(10+i*10)|0x20000000u;node.order=i;
        Put(node.bytes,0,i);Put(node.bytes,8,uint32_t(4));Put(node.bytes,16,uint64_t(0x10000+(i==3?1:i)*4096));
        Put(node.bytes,24,uint32_t(i>=3));Put(node.bytes,28,uint32_t(0xdead0000));
        node.vtable=Base+0x3094850;node.work=Base+0x332D5C;Put(node.definition,0,node.vtable);
        Put(node.definition,24,paired_frontier::Name(i==1 || i==3?"GBuffer_Discard":"Other"));source.nodes.push_back(node);
    }
    std::array<paired_frontier::Edges,2> edges;
    edges[0]={{10,20},{10,30},{20,40},{30,40},{40,50},{40,60}};
    edges[1]={{10,20},{20,30},{30,40},{40,50},{50,60}};
    OwnedNativeGraph graph;Require(graph.Create(source,edges) && graph.CompileReference());return graph.Snapshot();
}
std::array<scene_expansion::Definition,2> Definitions(const graph_snapshot::Snapshot& source,uint32_t index=1){
    return {{{0x50000,source.nodes[index].definition},{0x60000,source.nodes[index].definition}}};
}
void Positive(){
    const auto source=Fixture(),saved=source;const auto defs=Definitions(source);scene_expansion::Plan plan;
    Require(plan.Build(source,Base,source.nodes[1].handle,defs) && plan.Ready() && !plan.ExecutionPermitted());
    Require(source.nodes==saved.nodes && source.edges==saved.edges && plan.MatchesSource(source,Base));
    const auto& expanded=plan.Expanded();Require(expanded.nodes.size()==7 && plan.Mappings().size()==7);
    Require(plan.PrefixHandle()==source.nodes[1].handle && plan.TailHandle()!=plan.PrefixHandle());
    for(uint32_t i=0;i<source.nodes.size();++i){
        const auto& before=source.nodes[i];const auto& after=expanded.nodes[i];
        Require(after.handle==before.handle && after.order==before.order && after.definition==before.definition);
        for(size_t byte=0;byte<28;++byte)if(i!=1 || byte<16 || byte>=24)Require(after.bytes[byte]==before.bytes[byte]);
        Require(after.bytes[30]==0xad && after.bytes[31]==0xde);
        Require(plan.Mappings()[i]==scene_expansion::Mapping{before.handle,after.handle,i==1?1u:0u});
    }
    Require(graph_snapshot::Field<uint64_t>(expanded.nodes[3].bytes,16)==graph_snapshot::Field<uint64_t>(source.nodes[3].bytes,16));
    Require(graph_snapshot::Field<uint64_t>(expanded.nodes[1].bytes,16)==defs[0].object);
    Require(graph_snapshot::Field<uint64_t>(expanded.nodes.back().bytes,16)==defs[1].object);
    const auto tail=plan.TailHandle()&0xDFFFFFFFu;
    Require(plan.Edges()[0].contains({10,20}) && plan.Edges()[0].contains({20,tail}) && plan.Edges()[0].contains({tail,40}));
    Require(plan.Edges()[1].contains({20,tail}) && plan.Edges()[1].contains({tail,30}));
    OwnedNativeGraph actual;Require(actual.Create(expanded,plan.Edges()) && actual.CompileReference() && plan.MatchesCompiled(actual.Snapshot()));
    auto corrupt=expanded;corrupt.nodes.back().definition[24]^=1;Require(!plan.MatchesCompiled(corrupt));
    corrupt=source;corrupt.nodes[0].definition[8]^=1;Require(!plan.MatchesSource(corrupt,Base));
    Require(!plan.MatchesSource(source,Base+1));
    auto nativeIds=source;for(auto& edge:nativeIds.edges)edge.handle&=0xBFFFFFFFu;
    for(auto& node:nativeIds.nodes)node.handle&=0xDFFFFFFFu;
    Require(plan.Build(nativeIds,Base,nativeIds.nodes[1].handle,defs) && plan.Ready());
    // Native edge slots can move while the embedded ordinal remains unchanged.
    const auto remap=[](uint32_t value){return value==0x40000000u?0x40000002u:value==0x40000002u?0x40000000u:value;};
    std::swap(nativeIds.edges[0].handle,nativeIds.edges[2].handle);
    for(auto& node:nativeIds.nodes)for(const size_t at:{32u,36u,40u,44u})Put(node.bytes,at,remap(graph_snapshot::Field<uint32_t>(node.bytes,at)));
    for(auto& edge:nativeIds.edges)for(const size_t at:{16u,20u})Put(edge.bytes,at,remap(graph_snapshot::Field<uint32_t>(edge.bytes,at)));
    Require(plan.Build(nativeIds,Base,nativeIds.nodes[1].handle,defs) && plan.Ready());
}
void Refusal(){
    const auto source=Fixture();
    const auto reject=[&](auto mutation){auto bad=source;auto defs=Definitions(bad);mutation(bad,defs);scene_expansion::Plan plan;
        Require(!plan.Build(bad,Base,source.nodes[1].handle,defs) && !plan.Ready() && !plan.ExecutionPermitted());};
    reject([](auto& s,auto&){s.failure=1;});
    reject([](auto& s,auto&){s.nodes[1].work+=1;});
    reject([](auto& s,auto&){s.nodes[1].handle=7;});
    reject([](auto& s,auto&){Put(s.nodes[1].bytes,16,uint64_t(0));});
    reject([](auto& s,auto&){Put(s.nodes[1].bytes,16,uint64_t(0x11001));});
    reject([](auto& s,auto&){Put(s.nodes[1].bytes,16,UINT64_MAX-7);});
    reject([](auto& s,auto&){s.nodes[1].bytes[32]^=1;});
    reject([](auto& s,auto&){s.nodes[1].bytes[28]^=1;});
    reject([](auto& s,auto&){s.edges[0].bytes[16]^=1;});
    reject([](auto& s,auto&){s.edges[0].bytes[20]^=1;});
    reject([](auto& s,auto&){s.edges[0].bytes[4]=2;});
    reject([](auto& s,auto&){s.nodes[2].order=10;});
    reject([](auto&,auto& d){d[0].object=0;});
    reject([](auto&,auto& d){d[0].object+=1;});
    reject([](auto&,auto& d){d[0].object=UINT64_MAX-7;});
    reject([](auto&,auto& d){d[1].object=d[0].object;});
    reject([](auto&,auto& d){d[1].object=d[0].object+48;});
    reject([](auto& s,auto& d){d[0].object=graph_snapshot::Field<uint64_t>(s.nodes[3].bytes,16);});
    reject([](auto& s,auto& d){d[0].object=graph_snapshot::Field<uint64_t>(s.nodes[1].bytes,16)+32;});
    reject([](auto& s,auto& d){d[0].object=graph_snapshot::Field<uint64_t>(s.nodes[4].bytes,16)+40;});
    reject([](auto&,auto& d){d[0].prefix[8]^=1;});
    scene_expansion::Plan plan;Require(plan.Build(source,Base,source.nodes[1].handle,Definitions(source)));
    Require(!plan.Build(source,0,source.nodes[1].handle,Definitions(source)) && !plan.Ready() && plan.Expanded().nodes.empty());
}

void Declaration(){
    const auto source=Fixture();const auto definitions=Definitions(source);scene_expansion::Plan plan;
    Require(plan.Build(source,Base,source.nodes[1].handle,definitions));
    Require(plan.Source().nodes==source.nodes && plan.Source().edges==source.edges && plan.Definitions()==definitions);
    Require(plan.MatchesDeclaration(source,plan.Expanded(),Base));
    auto coupled=source,expanded=plan.Expanded();
    coupled.nodes[0].definition[8]^=1;expanded.nodes[0].definition=coupled.nodes[0].definition;
    Require(plan.MatchesDeclaration(coupled,expanded,Base));
    Require(!plan.MatchesSource(coupled,Base) && !plan.MatchesCompiled(expanded));
    Require(!plan.MatchesDeclaration(coupled,plan.Expanded(),Base) && !plan.MatchesDeclaration(source,expanded,Base));
    coupled.nodes[4].definition[16]^=1;expanded.nodes[4].definition=coupled.nodes[4].definition;
    Require(plan.MatchesDeclaration(coupled,expanded,Base));
    const auto reject=[&](auto mutate){auto left=source,right=plan.Expanded();mutate(left,right);
        Require(!plan.MatchesDeclaration(left,right,Base));};
    reject([](auto& left,auto&){left.failure=1;});reject([](auto&,auto& right){right.failure=1;});
    reject([](auto& left,auto&){left.nodes.pop_back();});reject([](auto&,auto& right){right.nodes.pop_back();});
    reject([](auto& left,auto& right){left.nodes[1].definition[8]^=1;right.nodes[1].definition=left.nodes[1].definition;right.nodes.back().definition=left.nodes[1].definition;});
    reject([](auto& left,auto& right){left.nodes[3].definition[8]^=1;right.nodes[3].definition=left.nodes[3].definition;});
    reject([](auto&,auto& right){right.nodes[1].definition[8]^=1;});
    reject([](auto&,auto& right){right.nodes.back().definition[8]^=1;});
    reject([](auto& left,auto& right){left.nodes[0].definition[0]^=1;right.nodes[0].definition=left.nodes[0].definition;});
    reject([](auto& left,auto& right){++left.nodes[0].vtable;right.nodes[0].vtable=left.nodes[0].vtable;});
    reject([](auto& left,auto& right){++left.nodes[0].work;right.nodes[0].work=left.nodes[0].work;});
    reject([](auto& left,auto& right){++left.nodes[0].handle;right.nodes[0].handle=left.nodes[0].handle;});
    reject([](auto& left,auto& right){++left.nodes[0].order;right.nodes[0].order=left.nodes[0].order;});
    reject([](auto& left,auto& right){left.nodes[0].bytes[24]^=1;right.nodes[0].bytes[24]^=1;});
    reject([](auto& left,auto& right){left.nodes[0].bytes[16]^=8;right.nodes[0].bytes[16]^=8;});
    reject([](auto&,auto& right){right.nodes[1].bytes[16]^=8;});
    reject([](auto&,auto& right){right.nodes.back().bytes[16]^=8;});
    reject([](auto& left,auto&){left.nodes[0].bytes[32]^=1;});
    reject([](auto&,auto& right){right.nodes[0].bytes[32]^=1;});
    reject([](auto& left,auto&){left.edges[0].bytes[8]^=1;});
    reject([](auto&,auto& right){right.edges[0].bytes[8]^=1;});
    reject([](auto& left,auto&){++left.edges[0].order;});
    reject([](auto&,auto& right){++right.edges[0].handle;});
    Require(!plan.MatchesDeclaration(source,plan.Expanded(),Base+1));
    Require(!scene_expansion::Plan{}.MatchesDeclaration(source,plan.Expanded(),Base));
    // Native untagged edge handles still identify slots, not embedded ordinals.
    auto native=source;for(auto& edge:native.edges)edge.handle&=0xBFFFFFFFu;
    Require(plan.Build(native,Base,native.nodes[1].handle,Definitions(native)));
    Require(plan.MatchesDeclaration(native,plan.Expanded(),Base));
}
template<class T>std::vector<T> Read(const char* path){
    std::ifstream file(path,std::ios::binary|std::ios::ate);Require(bool(file));const auto size=file.tellg();
    Require(size>=0 && size<=16*1024*1024 && size%sizeof(T)==0);std::vector<T> rows(size_t(size)/sizeof(T));
    file.seekg(0);file.read(reinterpret_cast<char*>(rows.data()),size);Require(bool(file));return rows;
}
}
int main(int argc,char** argv)try{
    Positive();Refusal();Declaration();Require(argc==1 || argc==3);
    if(argc==3){graph_snapshot::Snapshot source;source.nodes=Read<graph_snapshot::Node>(argv[1]);source.edges=Read<graph_snapshot::Edge>(argv[2]);
        bool tested{};for(uint32_t i=0;i<source.nodes.size();++i){const auto& node=source.nodes[i];
            if(graph_snapshot::Field<uint64_t>(node.definition,24)!=paired_frontier::Name("GBuffer_Discard"))continue;
            const auto base=node.work-0x332D5C;auto defs=Definitions(source,i);scene_expansion::Plan plan;
            if(!plan.Build(source,base,node.handle,defs)){
                std::fprintf(stderr,"Saved source expansion rejected: handle=%u, reason=%u\n",node.handle,uint32_t(plan.Rejection()));
                Require(false);
            }tested=true;
            std::printf("PASS saved native graph one-eye expansion: %zu -> %zu nodes, source handle %u, tail %u\n",
                source.nodes.size(),plan.Expanded().nodes.size(),plan.PrefixHandle(),plan.TailHandle());
        }Require(tested);
    }
    std::puts("PASS exact source-bound SceneDriver graph expansion, contraction and immutable other-eye identity");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
