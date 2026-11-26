#include "Render/ScenePrefixOrderPlan.hpp"
#include "Render/PairedCpuFrontier.hpp"
#include <cstdio>
#include <fstream>
#include <stdexcept>

using namespace cvr::stereo;
namespace {
constexpr uint64_t Base=0x140000000;
using Edges=paired_frontier::Edges;
void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
template<class T,size_t N>void Put(std::array<uint8_t,N>& bytes,size_t offset,T value){std::memcpy(bytes.data()+offset,&value,sizeof(value));}
graph_snapshot::Snapshot Compile(const graph_snapshot::Snapshot& source,const std::array<Edges,2>& edges){
    OwnedNativeGraph graph;Require(graph.Create(source,edges) && graph.CompileReference(),"Fixture compilation failed");return graph.Snapshot();
}
std::array<Edges,2> FixtureEdges(){
    const Edges chain{{10,30},{30,20},{20,40},{40,50},{50,60}};return {chain,chain};
}
graph_snapshot::Snapshot Fixture(uint32_t count=6){
    graph_snapshot::Snapshot source;
    for(uint32_t i=0;i<count;++i){graph_snapshot::Node node;node.handle=(10+i*10)|0x20000000u;node.order=i;
        Put(node.bytes,0,i);Put(node.bytes,8,uint32_t(4));Put(node.bytes,16,uint64_t(0x100000+(i==3?1:i)*64));
        Put(node.bytes,24,uint32_t(i<3));Put(node.bytes,28,uint32_t(0xdead0000));
        node.vtable=Base+0x3094850;node.work=Base+0x332D5C;Put(node.definition,0,node.vtable);
        Put(node.definition,24,paired_frontier::Name(i==1 || i==3?"GBuffer_Discard":"Other"));source.nodes.push_back(node);
    }
    return Compile(source,FixtureEdges());
}
scene_expansion::BothEyesPlan Expand(const graph_snapshot::Snapshot& source,uint64_t base=Base){
    std::array<std::array<scene_expansion::Definition,2>,2> definitions{};
    for(const auto& node:source.nodes)if(graph_snapshot::Field<uint64_t>(node.definition,24)==paired_frontier::Name("GBuffer_Discard")){
        const auto view=graph_snapshot::Field<uint32_t>(node.bytes,24);Require(view<2,"Invalid fixture view");
        for(uint32_t section=0;section<2;++section)definitions[view][section]={0x50000ull+view*0x10000ull+section*0x1000ull,node.definition};
    }
    scene_expansion::BothEyesPlan result;Require(result.Build(source,base,definitions),"Both-eye fixture expansion failed");return result;
}
std::set<std::pair<uint32_t,uint32_t>> Reachability(const graph_snapshot::Snapshot& source,const Edges& edges){
    std::set<std::pair<uint32_t,uint32_t>> result;
    for(const auto& from:source.nodes)for(const auto& to:source.nodes)if(from.handle!=to.handle &&
        !paired_frontier::detail::Path(edges,from.handle&0xDFFFFFFFu,to.handle&0xDFFFFFFFu).empty())
        result.emplace(from.handle&0xDFFFFFFFu,to.handle&0xDFFFFFFFu);
    return result;
}
void Check(const scene_expansion::BothEyesPlan& expansion,bool fullClosure=true){
    const auto saved=expansion.Expanded();scene_prefix_order::Plan plan;
    Require(plan.Build(expansion) && plan.Ready() && plan.Rejection()==scene_prefix_order::Failure::None &&
        !plan.ExecutionPermitted(),"Valid prefix-order candidate rejected or execution authorized");
    Require(expansion.Expanded().nodes==saved.nodes && expansion.Expanded().edges==saved.edges &&
        plan.Source().nodes==saved.nodes && plan.Source().edges==saved.edges,"Prefix-order candidate mutated its source");
    const auto& handles=plan.Handles();std::array<uint32_t,4> ids{},orders{};
    for(size_t i=0;i<handles.size();++i){ids[i]=handles[i]&0xDFFFFFFFu;
        const auto found=std::find_if(saved.nodes.begin(),saved.nodes.end(),[&](const auto& node){return node.handle==handles[i];});
        Require(found!=saved.nodes.end(),"Candidate handle missing");orders[i]=found->order;
    }
    const auto& candidate=plan.Candidate();Require(candidate.nodes.size()==saved.nodes.size(),"Candidate node count changed");
    for(uint32_t phase=0;phase<2;++phase){
        auto expected=expansion.Edges()[phase];Require(expected.erase({ids[1],ids[2]})==1,"Original adjacency edge missing");
        expected.emplace(ids[0],ids[2]);expected.emplace(ids[2],ids[1]);expected.emplace(ids[1],ids[3]);
        Require(expected==plan.Edges()[phase] && expected.size()==expansion.Edges()[phase].size()+2,"Candidate edge delta is not exact");
        const std::array<uint32_t,4> chain{ids[0],ids[2],ids[1],ids[3]};
        for(size_t i=1;i<chain.size();++i)Require(!paired_frontier::detail::Path(expected,chain[i-1],chain[i]).empty(),"Candidate order lacks a dependency path");
        for(size_t i=0;i<candidate.nodes.size();++i){
            const auto before=i==orders[1]?orders[2]:i==orders[2]?orders[1]:i;
            Require(candidate.nodes[i].bytes[28+phase]==saved.nodes[before].bytes[28+phase],"Unrelated phase rank changed");
        }
        if(fullClosure){auto before=Reachability(saved,expansion.Edges()[phase]);const auto after=Reachability(candidate,expected);
            Require(before.erase({ids[1],ids[2]})==1 && before.emplace(ids[2],ids[1]).second && before==after,
                "Candidate changed another transitive dependency");}
    }
    for(size_t i=0;i<candidate.nodes.size();++i){auto restored=candidate.nodes[i];
        restored.bytes[28]=saved.nodes[i].bytes[28];restored.bytes[29]=saved.nodes[i].bytes[29];
        std::copy(saved.nodes[i].bytes.begin()+32,saved.nodes[i].bytes.end(),restored.bytes.begin()+32);
        Require(restored==saved.nodes[i],"Candidate changed node payload or definitions");
    }
    OwnedNativeGraph reference;
    Require(reference.Create(candidate,plan.Edges()) && reference.CompileReference() && reference.Snapshot().nodes==candidate.nodes &&
        reference.Snapshot().edges==candidate.edges,"Candidate failed independent graph compilation");
}
void Positive(){
    Check(Expand(Fixture()));
    auto source=Fixture();for(auto& node:source.nodes)node.handle&=0xDFFFFFFFu;for(auto& edge:source.edges)edge.handle&=0xBFFFFFFFu;
    Check(Expand(source));
    const auto remap=[](uint32_t value){return value==0x40000000u?0x40000002u:value==0x40000002u?0x40000000u:value;};
    std::swap(source.edges[0].handle,source.edges[2].handle);
    for(auto& node:source.nodes)for(const size_t at:{32u,36u,40u,44u})Put(node.bytes,at,remap(graph_snapshot::Field<uint32_t>(node.bytes,at)));
    for(auto& edge:source.edges)for(const size_t at:{16u,20u})Put(edge.bytes,at,remap(graph_snapshot::Field<uint32_t>(edge.bytes,at)));
    Check(Expand(source));
    Check(Expand(Fixture(218)),false);
    scene_prefix_order::Plan plan;Require(plan.Build(Expand(Fixture())),"Plan initialization failed");
    Require(!plan.Build({}) && !plan.Ready() && plan.Rejection()==scene_prefix_order::Failure::Source &&
        plan.Source().nodes.empty() && plan.Candidate().nodes.empty() && !plan.ExecutionPermitted(),"Failed rebuild retained candidate");
}
void Refusal(){
    const auto source=Fixture();
    const auto reject=[&](const Edges& bad,scene_prefix_order::Failure expected){
        for(uint32_t phase=0;phase<2;++phase){auto edges=FixtureEdges();edges[phase]=bad;
            const auto expansion=Expand(Compile(source,edges));scene_prefix_order::Plan plan;
            Require(!plan.Build(expansion) && !plan.Ready() && !plan.ExecutionPermitted() && plan.Rejection()==expected,
                "Adversarial edge topology was accepted or misclassified");}
    };
    reject({{10,20},{20,30},{30,40},{40,50},{50,60}},scene_prefix_order::Failure::Edges);
    reject({{10,20},{20,30},{30,40},{20,40},{40,50},{50,60}},scene_prefix_order::Failure::Cycle);
    reject({{10,20},{20,40},{10,50},{50,60},{60,30}},scene_prefix_order::Failure::Ranks);
    // Both adjacent ranks still swap cleanly, but an independent MAIN producer
    // would newly precede peer tail. Preserved nonselected ranks are not enough.
    reject({{10,20},{10,30},{20,40},{30,40},{40,50},{50,60}},scene_prefix_order::Failure::Reachability);
    // A peer-only consumer would newly depend on MAIN prefix.
    reject({{10,30},{30,20},{20,40},{20,50},{40,60}},scene_prefix_order::Failure::Reachability);
    auto ambiguous=source;ambiguous.nodes[3].handle=ambiguous.nodes[1].handle;
    std::array<std::array<scene_expansion::Definition,2>,2> definitions{};
    for(uint32_t view=0;view<2;++view)for(uint32_t section=0;section<2;++section)
        definitions[view][section]={0x50000ull+view*0x10000ull+section*0x1000ull,source.nodes[1].definition};
    scene_expansion::BothEyesPlan invalid;
    Require(!invalid.Build(ambiguous,Base,definitions),"Ambiguous native handles entered BOTH expansion");
    scene_prefix_order::Plan plan;Require(!plan.Build(invalid) && plan.Rejection()==scene_prefix_order::Failure::Source,"Invalid expansion became a candidate");
}
template<class T>std::vector<T> Read(const char* path){
    std::ifstream file(path,std::ios::binary|std::ios::ate);Require(bool(file),"Saved graph missing");const auto bytes=file.tellg();
    Require(bytes>=0 && bytes<=16*1024*1024 && bytes%sizeof(T)==0,"Saved graph ABI/count invalid");
    std::vector<T> values(size_t(bytes)/sizeof(T));file.seekg(0);file.read(reinterpret_cast<char*>(values.data()),bytes);
    Require(bool(file),"Saved graph truncated");return values;
}
void Saved(const char* nodes,const char* edges){
    graph_snapshot::Snapshot source;source.nodes=Read<graph_snapshot::Node>(nodes);source.edges=Read<graph_snapshot::Edge>(edges);
    const auto selected=std::find_if(source.nodes.begin(),source.nodes.end(),[](const auto& node){
        return graph_snapshot::Field<uint64_t>(node.definition,24)==paired_frontier::Name("GBuffer_Discard");});
    Require(selected!=source.nodes.end(),"Saved discard node missing");const auto base=selected->work-0x332D5C;
    paired_frontier::Plan pass;
    Require(pass.BuildCandidate(source,base,paired_frontier::Boundary::Discard)==paired_frontier::Result::CandidateNeedsCpuAndResourceCompile,"Saved pass frontier rejected");
    paired_cpu_frontier::Plan cpu;
    Require(cpu.BuildCandidate(pass)==paired_cpu_frontier::Result::CandidateNeedsOwnershipAndResourceCompile,"Saved CPU frontier rejected");
    const auto both=Expand(Compile(source,cpu.Proposed()),base);Check(both);
    scene_prefix_order::Plan plan;Require(plan.Build(both),"Saved prefix candidate rejected");const auto& h=plan.Handles();
    std::printf("PASS saved native prefix candidate %u,%u,%u,%u -> %u,%u,%u,%u; %zu nodes unchanged; execution permission false\n",
        h[0],h[1],h[2],h[3],h[0],h[2],h[1],h[3],plan.Candidate().nodes.size());
}
}
int main(int argc,char** argv)try{
    Require(argc==1 || argc==3,"scene_prefix_order_plan_test [original nodes original edges]");Positive();Refusal();
    if(argc==3)Saved(argv[1],argv[2]);
    std::puts("PASS source-bound prefix-order candidate, exact dependency swap, immutable payload and adversarial cycle/rank checks");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
