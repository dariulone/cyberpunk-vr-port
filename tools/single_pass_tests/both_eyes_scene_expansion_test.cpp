#include "Render/BothEyesSceneExpansion.hpp"
#include "Render/PairedCpuFrontier.hpp"
#include <cstdio>
#include <fstream>
#include <stdexcept>
#include <string>

using namespace cvr::stereo;
namespace {
constexpr uint64_t Base=0x140000000;
using Definitions=std::array<std::array<scene_expansion::Definition,2>,2>;
void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
template<class T,size_t N>void Put(std::array<uint8_t,N>& bytes,size_t offset,T value){std::memcpy(bytes.data()+offset,&value,sizeof(value));}
std::array<paired_frontier::Edges,2> FixtureEdges(){
    return {paired_frontier::Edges{{10,20},{10,30},{20,40},{30,40},{40,50},{40,60}},
            paired_frontier::Edges{{10,20},{20,30},{30,40},{40,50},{50,60}}};
}
graph_snapshot::Snapshot Compile(const graph_snapshot::Snapshot& source,const std::array<paired_frontier::Edges,2>& edges){
    OwnedNativeGraph graph;
    Require(graph.Create(source,edges) && graph.CompileReference(),"Fixture did not compile");return graph.Snapshot();
}
graph_snapshot::Snapshot Fixture(bool shared=true,uint32_t count=6){
    graph_snapshot::Snapshot source;
    for(uint32_t i=0;i<count;++i){graph_snapshot::Node node;node.handle=(10+i*10)|0x20000000u;node.order=i;
        Put(node.bytes,0,i);Put(node.bytes,8,uint32_t(4));
        Put(node.bytes,16,i<6?uint64_t(0x10000+(shared && i==3?1:i)*4096):uint64_t(0x100000+i*64));
        Put(node.bytes,24,uint32_t(i<3));Put(node.bytes,28,uint32_t(0xdead0000));
        node.vtable=Base+0x3094850;node.work=Base+0x332D5C;Put(node.definition,0,node.vtable);
        Put(node.definition,24,paired_frontier::Name(i==1 || i==3?"GBuffer_Discard":"Other"));source.nodes.push_back(node);
    }
    return Compile(source,FixtureEdges());
}
Definitions MakeDefinitions(const graph_snapshot::Snapshot& source){
    Definitions result{};
    for(const auto& node:source.nodes){
        if(graph_snapshot::Field<uint64_t>(node.definition,24)!=paired_frontier::Name("GBuffer_Discard"))continue;
        const auto view=graph_snapshot::Field<uint32_t>(node.bytes,24);
        Require(view<2,"Fixture has invalid eye");
        for(uint32_t section=0;section<2;++section)
            result[view][section]={0x50000ull+(view*2+section)*0x10000ull,node.definition};
    }
    return result;
}
const graph_snapshot::Node& Node(const graph_snapshot::Snapshot& source,uint32_t handle){
    const auto found=std::find_if(source.nodes.begin(),source.nodes.end(),[&](const auto& node){return node.handle==handle;});
    Require(found!=source.nodes.end(),"Missing graph node");return *found;
}
void Check(const graph_snapshot::Snapshot& source,uint64_t base){
    const auto saved=source;const auto definitions=MakeDefinitions(source);scene_expansion::BothEyesPlan plan;
    Require(plan.Build(source,base,definitions) && plan.Ready() && !plan.ExecutionPermitted(),"Both-eye expansion rejected");
    Require(source.nodes==saved.nodes && source.edges==saved.edges,"Expansion mutated original source");
    Require(plan.Source().nodes==source.nodes && plan.Source().edges==source.edges && plan.MatchesSource(source,base),"Original source not retained");
    Require(plan.Expanded().nodes.size()==source.nodes.size()+2 && plan.Mappings().size()==source.nodes.size()+2,"Both tails were not added");
    Require(plan.MatchesCompiled(plan.Expanded()) && plan.MatchesDeclaration(source,plan.Expanded(),base),"Fresh expansion did not validate");
    std::set<uint32_t> handles;
    for(uint32_t view=0;view<2;++view){
        Require(plan.Definitions(view)==definitions[view] && handles.insert(plan.PrefixHandle(view)).second &&
            handles.insert(plan.TailHandle(view)).second,"Clone definitions/handles overlap");
        Require(plan.SelectedSource(view)==Node(source,plan.PrefixHandle(view)),"Selected source was captured after rank mutation");
    }
    std::array<uint32_t,3> sections{};
    for(const auto& mapping:plan.Mappings()){
        Require(mapping.section<sections.size(),"Invalid flattened section");++sections[mapping.section];
        const auto& original=Node(source,mapping.sourceHandle);const auto& actual=Node(plan.Expanded(),mapping.executionHandle);
        if(!mapping.section){
            Require(actual.handle==original.handle && actual.definition==original.definition &&
                graph_snapshot::Field<uint64_t>(actual.bytes,16)==graph_snapshot::Field<uint64_t>(original.bytes,16),"Identity node changed");
        }else{
            const auto view=graph_snapshot::Field<uint32_t>(original.bytes,24);
            Require(view<2 && mapping.sourceHandle==plan.PrefixHandle(view) &&
                graph_snapshot::Field<uint64_t>(actual.bytes,16)==definitions[view][mapping.section-1].object &&
                actual.definition==definitions[view][mapping.section-1].prefix,"Flattened mapping lost original eye");
        }
    }
    Require(sections==std::array<uint32_t,3>{uint32_t(source.nodes.size()-2),2,2},"Wrong flattened mapping coverage");
    for(uint32_t phase=0;phase<2;++phase){
        const std::array<uint32_t,4> chain{plan.PrefixHandle(1),plan.TailHandle(1),plan.PrefixHandle(0),plan.TailHandle(0)};
        for(size_t i=1;i<chain.size();++i){
            Require(Node(plan.Expanded(),chain[i-1]).bytes[28+phase]<Node(plan.Expanded(),chain[i]).bytes[28+phase],"Eye order changed");
            Require(!paired_frontier::detail::Path(plan.Edges()[phase],chain[i-1]&0xDFFFFFFFu,chain[i]&0xDFFFFFFFu).empty(),"Eye order has no dependency path");
        }
        paired_frontier::Edges original,contracted;
        for(const auto& edge:source.edges)if(graph_snapshot::Field<uint32_t>(edge.bytes,4)==phase)
            original.emplace(graph_snapshot::Field<uint32_t>(edge.bytes,8)&0xDFFFFFFFu,graph_snapshot::Field<uint32_t>(edge.bytes,12)&0xDFFFFFFFu);
        std::map<uint32_t,uint32_t> inverse;
        for(const auto& mapping:plan.Mappings())inverse.emplace(mapping.executionHandle&0xDFFFFFFFu,mapping.sourceHandle&0xDFFFFFFFu);
        uint32_t internal{};
        for(const auto& [from,to]:plan.Edges()[phase]){
            const auto a=inverse.at(from),b=inverse.at(to);
            if(a==b){++internal;continue;}Require(contracted.emplace(a,b).second,"Contraction merged unrelated edges");
        }
        Require(internal==2 && contracted==original,"Inverse contraction did not reproduce source edges");
    }
    OwnedNativeGraph compiled;
    Require(compiled.Create(plan.Expanded(),plan.Edges()) && compiled.CompileReference() && plan.MatchesCompiled(compiled.Snapshot()),"Final graph compilation differs");
}
void Positive(){
    Check(Fixture(),Base);Check(Fixture(false),Base);
    auto source=Fixture();for(auto& node:source.nodes)node.handle&=0xDFFFFFFFu;for(auto& edge:source.edges)edge.handle&=0xBFFFFFFFu;
    Check(source,Base);
    // Native edge handles are pool slots, not edge ordinals.
    const auto remap=[](uint32_t value){return value==0x40000000u?0x40000002u:value==0x40000002u?0x40000000u:value;};
    std::swap(source.edges[0].handle,source.edges[2].handle);
    for(auto& node:source.nodes)for(const size_t at:{32u,36u,40u,44u})Put(node.bytes,at,remap(graph_snapshot::Field<uint32_t>(node.bytes,at)));
    for(auto& edge:source.edges)for(const size_t at:{16u,20u})Put(edge.bytes,at,remap(graph_snapshot::Field<uint32_t>(edge.bytes,at)));
    Check(source,Base);
    source=Fixture();scene_expansion::Plan peer;
    Require(peer.Build(source,Base,source.nodes[1].handle,MakeDefinitions(source)[1]),"Intermediate peer plan failed");
    Require(source.nodes[3].bytes[28]!=peer.Expanded().nodes[3].bytes[28],"Fixture does not expose intermediate rank mutation");
    auto relocated=Fixture();constexpr uint64_t delta=0x200000000;
    for(auto& node:relocated.nodes){node.vtable+=delta;node.work+=delta;Put(node.definition,0,node.vtable);}Check(relocated,Base+delta);
    Check(Fixture(true,218),Base);
    scene_expansion::BothEyesPlan plan;source=Fixture(true,219);
    Require(!plan.Build(source,Base,MakeDefinitions(source)) && plan.Rejection()==scene_expansion::Failure::Capacity,"N+2 capacity overflow accepted");
}
void Refusal(){
    const auto source=Fixture();
    const auto reject=[&](auto mutation){auto bad=source;auto definitions=MakeDefinitions(bad);mutation(bad,definitions);scene_expansion::BothEyesPlan plan;
        Require(!plan.Build(bad,Base,definitions) && !plan.Ready() && !plan.ExecutionPermitted(),"Invalid expansion accepted");};
    reject([](auto& s,auto&){s.failure=1;});
    reject([](auto& s,auto&){s.nodes.clear();});
    reject([](auto& s,auto&){s.nodes[1].work+=1;});
    reject([](auto& s,auto&){s.nodes[3].vtable+=8;});
    reject([](auto& s,auto&){Put(s.nodes[3].definition,0,uint64_t(0));});
    reject([](auto& s,auto&){Put(s.nodes[1].bytes,24,uint32_t(0));});
    reject([](auto& s,auto&){Put(s.nodes[1].bytes,24,uint32_t(2));});
    reject([](auto& s,auto&){Put(s.nodes[2].definition,24,paired_frontier::Name("GBuffer_Discard"));});
    reject([](auto& s,auto&){Put(s.nodes[1].definition,24,paired_frontier::Name("Other"));});
    reject([](auto& s,auto& d){s.nodes[3].definition[8]^=1;d[0][0].prefix=d[0][1].prefix=s.nodes[3].definition;});
    reject([](auto& s,auto&){Put(s.nodes[3].bytes,16,uint64_t(0));});
    reject([](auto& s,auto&){Put(s.nodes[3].bytes,16,uint64_t(0x11001));});
    reject([](auto& s,auto&){s.nodes[3].bytes[28]^=1;});
    reject([](auto& s,auto&){s.nodes[3].bytes[29]^=1;});
    reject([](auto& s,auto&){s.nodes[0].bytes[32]^=1;});
    reject([](auto& s,auto&){s.edges[0].bytes[16]^=1;});
    reject([](auto& s,auto&){s.edges[0].bytes[20]^=1;});
    reject([](auto&,auto& d){d[0][0].object=0;});
    reject([](auto&,auto& d){d[1][0].object+=1;});
    reject([](auto&,auto& d){d[0][1].object=UINT64_MAX-7;});
    reject([](auto&,auto& d){d[0][1].object=d[0][0].object;});
    reject([](auto&,auto& d){d[1][0].object=d[0][0].object+48;});
    reject([](auto&,auto& d){d[1][1].object=d[0][1].object-48;});
    reject([](auto& s,auto& d){d[0][0].object=graph_snapshot::Field<uint64_t>(s.nodes[1].bytes,16);});
    reject([](auto& s,auto& d){d[0][1].object=graph_snapshot::Field<uint64_t>(s.nodes[5].bytes,16)+40;});
    reject([](auto&,auto& d){d[1][0].prefix[8]^=1;});
    reject([](auto&,auto& d){d[0][1].prefix[16]^=1;});
    // The peer root disappears from S1 when it is not shared. It is still a
    // forbidden overlap because all four clones are admitted against S0.
    auto separate=Fixture(false);auto definitions=MakeDefinitions(separate);scene_expansion::BothEyesPlan plan;
    definitions[0][0].object=graph_snapshot::Field<uint64_t>(separate.nodes[1].bytes,16)+40;
    Require(!plan.Build(separate,Base,definitions),"Removed original root overlap accepted");
    for(uint32_t phase=0;phase<2;++phase){
        auto edges=FixtureEdges();edges[phase].erase({20,40});edges[phase].erase({30,40});
        const auto disconnected=Compile(source,edges);
        Require(!plan.Build(disconnected,Base,MakeDefinitions(disconnected)),"Uncoupled eye order accepted");
        edges=FixtureEdges();edges[phase].erase({20,40});edges[phase].erase({30,40});
        edges[phase].emplace(10,70);edges[phase].emplace(70,80);edges[phase].emplace(80,40);
        if(phase==1)edges[phase].erase({20,30});
        const auto independent=Compile(Fixture(true,8),edges);
        scene_expansion::Plan peer,main;
        const auto defs=MakeDefinitions(independent);
        Require(peer.Build(independent,Base,independent.nodes[1].handle,defs[1]) &&
            main.Build(peer.Expanded(),Base,independent.nodes[3].handle,defs[0]),"Independent rank fixture failed");
        Require(Node(main.Expanded(),peer.TailHandle()).bytes[28+phase]<Node(main.Expanded(),main.PrefixHandle()).bytes[28+phase],
            "Independent rank fixture does not preserve incidental eye order");
        Require(!plan.Build(independent,Base,defs),"Incidental rank ordering without dependency path accepted");
    }
    auto reverse=source;for(auto& node:reverse.nodes)Put(node.bytes,24,graph_snapshot::Field<uint32_t>(node.bytes,24)^1);
    Require(!plan.Build(reverse,Base,MakeDefinitions(reverse)),"MAIN-before-peer source accepted");
    Require(plan.Build(source,Base,MakeDefinitions(source)),"Valid plan failed before reset");
    Require(!plan.Build(source,0,MakeDefinitions(source)) && !plan.Ready() && plan.Expanded().nodes.empty(),"Failed rebuild retained executable data");
}
void Declaration(){
    for(const bool shared:{false,true}){
        const auto source=Fixture(shared);scene_expansion::BothEyesPlan plan;
        Require(plan.Build(source,Base,MakeDefinitions(source)),"Declaration plan failed");
        auto current=source,expanded=plan.Expanded();
        current.nodes[0].definition[8]^=1;expanded.nodes[0].definition=current.nodes[0].definition;
        current.nodes[4].definition[16]^=1;expanded.nodes[4].definition=current.nodes[4].definition;
        Require(plan.MatchesDeclaration(current,expanded,Base),"Dynamic identity prefix failed intermediate refresh");
        Require(!plan.MatchesSource(current,Base) && !plan.MatchesCompiled(expanded),"Strict match admitted dynamic prefixes");
        Require(!plan.MatchesDeclaration(current,plan.Expanded(),Base) && !plan.MatchesDeclaration(source,expanded,Base),"Unmatched identity prefix accepted");
        const auto reject=[&](auto mutation){auto left=source,right=plan.Expanded();mutation(left,right);
            Require(!plan.MatchesDeclaration(left,right,Base),"Corrupt declaration accepted");};
        for(const uint32_t view:{0u,1u}){
            const auto selected=plan.SelectedSource(view).order;
            reject([&](auto& left,auto& right){left.nodes[selected].definition[8]^=1;right.nodes[selected].definition=left.nodes[selected].definition;});
            for(const auto handle:{plan.PrefixHandle(view),plan.TailHandle(view)}){
                const auto index=Node(plan.Expanded(),handle).order;
                reject([&](auto&,auto& right){right.nodes[index].definition[8]^=1;});
                reject([&](auto&,auto& right){right.nodes[index].bytes[16]^=8;});
                reject([&](auto&,auto& right){right.nodes[index].bytes[28]^=1;});
                reject([&](auto&,auto& right){right.nodes[index].bytes[29]^=1;});
            }
        }
        reject([](auto& left,auto&){left.failure=1;});reject([](auto&,auto& right){right.failure=1;});
        reject([](auto& left,auto&){left.nodes.pop_back();});reject([](auto&,auto& right){right.nodes.pop_back();});
        reject([](auto& left,auto& right){left.nodes[0].definition[0]^=1;right.nodes[0].definition=left.nodes[0].definition;});
        reject([](auto& left,auto& right){++left.nodes[0].work;++right.nodes[0].work;});
        reject([](auto& left,auto& right){++left.nodes[0].handle;++right.nodes[0].handle;});
        reject([](auto& left,auto& right){left.nodes[0].bytes[16]^=8;right.nodes[0].bytes[16]^=8;});
        reject([](auto& left,auto&){left.nodes[0].bytes[32]^=1;});
        reject([](auto&,auto& right){right.nodes[0].bytes[32]^=1;});
        reject([](auto& left,auto&){left.edges[0].bytes[8]^=1;});
        reject([](auto&,auto& right){right.edges[0].bytes[8]^=1;});
        reject([](auto&,auto& right){++right.edges[0].handle;});
        Require(!plan.MatchesDeclaration(source,plan.Expanded(),Base+1),"Wrong image base accepted");
    }
    Require(!scene_expansion::BothEyesPlan{}.MatchesDeclaration({}, {},Base),"Empty plan accepted declaration");
}
template<class T>std::vector<T> Read(const char* path){
    std::ifstream file(path,std::ios::binary|std::ios::ate);Require(bool(file),"Saved graph missing");const auto size=file.tellg();
    Require(size>=0 && size<=16*1024*1024 && size%sizeof(T)==0,"Saved graph ABI/count invalid");
    std::vector<T> rows(size_t(size)/sizeof(T));file.seekg(0);file.read(reinterpret_cast<char*>(rows.data()),size);
    Require(bool(file),"Saved graph truncated");return rows;
}
void Saved(const char* nodes,const char* edges){
    graph_snapshot::Snapshot source;source.nodes=Read<graph_snapshot::Node>(nodes);source.edges=Read<graph_snapshot::Edge>(edges);
    const auto found=std::find_if(source.nodes.begin(),source.nodes.end(),[](const auto& node){
        return graph_snapshot::Field<uint64_t>(node.definition,24)==paired_frontier::Name("GBuffer_Discard");});
    Require(found!=source.nodes.end(),"Saved discard source missing");const auto base=found->work-0x332D5C;
    paired_frontier::Plan pass;
    Require(pass.BuildCandidate(source,base,paired_frontier::Boundary::Discard)==paired_frontier::Result::CandidateNeedsCpuAndResourceCompile,"Saved pass frontier rejected");
    paired_cpu_frontier::Plan cpu;
    Require(cpu.BuildCandidate(pass)==paired_cpu_frontier::Result::CandidateNeedsOwnershipAndResourceCompile,"Saved coupled frontier rejected");
    const auto coupled=Compile(source,cpu.Proposed());Check(coupled,base);
    std::printf("PASS saved native coupled graph both-eye expansion: %zu -> %zu nodes\n",source.nodes.size(),source.nodes.size()+2);
}
}
int main(int argc,char** argv)try{
    Require(argc==1 || argc==3,"both_eyes_scene_expansion_test [nodes edges]");Positive();Refusal();Declaration();
    if(argc==3)Saved(argv[1],argv[2]);
    std::puts("PASS source-bound both-eye SceneDriver expansion, contraction, native order and dynamic declarations");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
