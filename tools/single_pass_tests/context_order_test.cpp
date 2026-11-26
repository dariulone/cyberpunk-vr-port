#include "Render/NativeContextOrder.hpp"
#include "Render/OwnedNativeGraph.hpp"
#include "Render/PairedCpuFrontier.hpp"
#include "Render/SceneGraphExpansion.hpp"
#include "Render/BothEyesSceneExpansion.hpp"
#include "cpu_preparation_overlap.hpp"
#include <cstdio>
#include <fstream>
#include <stdexcept>
#include <string>

namespace order=cvr::stereo::context_order;
namespace graph=cvr::stereo::graph_snapshot;
namespace frontier=cvr::stereo::paired_frontier;
constexpr uint64_t Base=0x140000000;
static uint64_t imageBase=Base;
static bool corruptLeaf{},unreadable{};
static void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
template<class T,size_t N>static void Put(std::array<uint8_t,N>& bytes,size_t offset,T value){std::memcpy(bytes.data()+offset,&value,sizeof(value));}
// Unit-test memory models the three audited leaf functions, not the game's live image.
static bool Read(uint64_t address,void* out,size_t bytes){
    if(unreadable || address<imageBase)return false;
    const auto rva=address-imageBase;
    if(bytes==8){
        uint64_t method=imageBase+0xADF4E0;
        if(rva==0x3094850+0x28)method=imageBase+0x1941AC0;
        else if(rva==0x36CED58+0x28)method=imageBase+0x1942080;
        else if(rva<0x30946F8 || rva>0x36D1DC0)return false;
        std::memcpy(out,&method,8);return true;
    }
    std::array<uint8_t,6> code{};
    if(rva==0x1941AC0 && bytes==6)code={0xB8,2,0,0,0,0xC3};
    else if(rva==0x1942080 && bytes==6)code={0xB8,3,0,0,0,0xC3};
    else if(rva==0xADF4E0 && bytes==3)code={0x33,0xC0,0xC3};
    else return false;
    if(corruptLeaf)code[0]^=1;std::memcpy(out,code.data(),bytes);return true;
}
struct Fixture {
    graph::Snapshot source;
    std::array<frontier::Edges,2> edges;
    Fixture(){
        const uint32_t vtables[]{0x3094850,0x36CED58,0x3094850,0x36CED58};
        for(uint32_t i=0;i<4;++i){graph::Node node;node.handle=node.order=i;
            Put(node.bytes,0,i);Put(node.bytes,8,uint32_t(4));Put(node.bytes,16,uint64_t(0x10000+i*64));
            Put(node.bytes,24,i/2);node.vtable=Base+vtables[i];node.work=Base+(i%2?0x655FAC:0x332D5C);
            Put(node.definition,0,node.vtable);Put(node.definition,24,i%2?uint32_t(2):uint32_t(7));source.nodes.push_back(node);
        }
        for(auto& phase:edges)for(uint32_t i=1;i<4;++i)phase.emplace(i-1,i);
    }
    graph::Snapshot Compile(){
        cvr::stereo::OwnedNativeGraph compiled;
        Require(compiled.Create(source,edges) && compiled.CompileReference(),"Fixture compilation failed");return compiled.Snapshot();
    }
};
template<class T>static std::vector<T> File(const char* path){
    std::ifstream input(path,std::ios::binary|std::ios::ate);Require(bool(input),"Saved graph missing");const auto size=input.tellg();
    Require(size>=0 && uint64_t(size)%sizeof(T)==0 && uint64_t(size)<=64*1024*1024,"Saved ABI/count invalid");
    std::vector<T> result(size_t(size)/sizeof(T));input.seekg(0);if(size)input.read(reinterpret_cast<char*>(result.data()),size);
    Require(bool(input),"Saved graph truncated");return result;
}
static void Saved(const char* nodes,const char* edges,uint64_t base){
    imageBase=base;graph::Snapshot source;source.nodes=File<graph::Node>(nodes);source.edges=File<graph::Edge>(edges);
    const auto original=order::Analyze(source,base,Read);
    Require(original.PrefixReady() && original.publishers==75 && original.synchronizers==13,"Saved native prefix readiness failed");
    frontier::Plan pass;
    Require(pass.BuildCandidate(source,base,frontier::Boundary::Discard)==frontier::Result::CandidateNeedsCpuAndResourceCompile,"Whole pass plan rejected");
    cvr::stereo::OwnedNativeGraph phaseOnly;
    Require(phaseOnly.Create(source,pass.Proposed()) && phaseOnly.CompileReference(),"Phase-only candidate failed to compile");
    const auto unsafe=order::Analyze(phaseOnly.Snapshot(),base,Read);
    Require(unsafe.failure==order::Failure::UnpublishedPrefix && !unsafe.missing.empty(),"Unready native context prefix escaped rejection");
    cvr::stereo::paired_cpu_frontier::Plan cpu;
    Require(cpu.BuildCandidate(pass)==cvr::stereo::paired_cpu_frontier::Result::CandidateNeedsOwnershipAndResourceCompile,"Coupled candidate rejected");
    cvr::stereo::OwnedNativeGraph coupled;
    Require(coupled.Create(source,cpu.Proposed()) && coupled.CompileReference(),"Coupled candidate failed to compile");
    const auto ready=order::Analyze(coupled.Snapshot(),base,Read);
    Require(ready.PrefixReady() && ready.requiredPairs==ready.provedPairs && !cpu.ExecutionReady(),"Coupled prefix readiness or nonexecution boundary failed");
    const auto coupledSource=coupled.Snapshot();
    for(const auto& node:coupledSource.nodes){
        if(graph::Field<uint64_t>(node.definition,24)!=frontier::Name("GBuffer_Discard"))continue;
        const std::array<cvr::stereo::scene_expansion::Definition,2> definitions{{{0x50000,node.definition},{0x60000,node.definition}}};
        cvr::stereo::scene_expansion::Plan expansion;
        Require(expansion.Build(coupledSource,base,node.handle,definitions),"Scene section expansion rejected source topology");
        const auto split=order::Analyze(expansion.Expanded(),base,Read);
        std::printf("One-eye graph split view%u: prefix %u/%u, publishers %u, missing %zu\n",
            graph::Field<uint32_t>(node.bytes,24),split.provedPairs,split.requiredPairs,split.publishers,split.missing.size());
        Require(split.PrefixReady() && split.publishers==ready.publishers+1 && split.synchronizers==ready.synchronizers,
            "One-eye split lost native context producer readiness");
    }
    std::array<std::array<cvr::stereo::scene_expansion::Definition,2>,2> bothDefinitions;
    for(const auto& node:coupledSource.nodes){
        if(graph::Field<uint64_t>(node.definition,24)!=frontier::Name("GBuffer_Discard"))continue;
        const auto view=graph::Field<uint32_t>(node.bytes,24);
        for(uint32_t section=0;section<2;++section)bothDefinitions[view][section]={0x70000+uint64_t(view*2+section)*0x1000,node.definition};
    }
    cvr::stereo::scene_expansion::BothEyesPlan both;
    Require(both.Build(coupledSource,base,bothDefinitions),"Both-eye scene expansion rejected source topology");
    const auto bothReady=order::Analyze(both.Expanded(),base,Read);
    std::printf("Both-eye graph split: prefix %u/%u, publishers %u, missing %zu\n",
        bothReady.provedPairs,bothReady.requiredPairs,bothReady.publishers,bothReady.missing.size());
    Require(bothReady.PrefixReady() && bothReady.publishers==ready.publishers+2 && bothReady.synchronizers==ready.synchronizers,
        "Both-eye split lost native context producer readiness");
    auto incomplete=cpu.Proposed();
    Require(incomplete[0].erase({pass.EyeRoles()[1].solidReady,pass.EyeRoles()[1].discard})==1,"Readiness edge absent from corrected candidate");
    cvr::stereo::OwnedNativeGraph missingReady;
    Require(missingReady.Create(source,incomplete) && missingReady.CompileReference(),"Incomplete candidate compilation failed");
    const auto rejected=order::Analyze(missingReady.Snapshot(),base,Read);
    Require(rejected.failure==order::Failure::UnpublishedPrefix && rejected.missing.size()==36,
        "Missing coupled solid/discard CPU prerequisite escaped rejection");
    std::printf("Saved prefix checks source %u/%u, phase-only missing %zu, coupled %u/%u; execution permission remains false\n",
        original.provedPairs,original.requiredPairs,unsafe.missing.size(),ready.provedPairs,ready.requiredPairs);
    frontier::Plan solid;
    Require(solid.BuildCandidate(source,base)==frontier::Result::CandidateNeedsCpuAndResourceCompile,"Solid frontier rejected");
    cvr::stereo::cpu_overlap::Plan overlap;const auto admitted=overlap.Build(solid,Read);
    std::printf("CPU-only overlap: candidate=%u reason=%u shared=%zu restored=%zu prefix=%zu GPUedgesUnchanged=%u; witness:",
        admitted,uint32_t(overlap.Rejection()),overlap.SharedOrders().size(),overlap.RestoredOrders().size(),overlap.PrefixOrders().size(),
        overlap.Proposed()[1]==overlap.Original()[1]);
    for(const auto id:overlap.Witness())std::printf(" %u",id);std::puts("");
    if(admitted){cvr::stereo::OwnedNativeGraph graph;
        Require(graph.Create(source,overlap.Proposed()) && graph.CompileReference(),"CPU overlap did not compile");
        const auto snapshot=graph.Snapshot();
        for(size_t i=0;i<source.nodes.size();++i)Require(source.nodes[i].bytes[29]==snapshot.nodes[i].bytes[29],"CPU overlap changed native GPU rank");
        Require(order::Analyze(snapshot,base,Read).PrefixReady(),"CPU overlap lost context publication");
    }else{Require(overlap.Rejection()==cvr::stereo::cpu_overlap::Failure::Cycle && overlap.Witness().size()>2,"Unexpected CPU overlap rejection");
        const auto& witness=overlap.Witness();Require(witness.front()==witness.back(),"Overlap rejection has no closed cycle");
        for(size_t i=1;i<witness.size();++i)Require(overlap.Proposed()[0].contains({witness[i-1],witness[i]}),"Overlap cycle contains a nonexistent edge");}
}
int main(int argc,char** argv)try{
    Require(argc==1 || argc==4,"context_order_test [nodes edges imageBase]");Fixture fixture;auto source=fixture.Compile();
    const auto complete=order::Analyze(source,Base,Read);
    Require(complete.PrefixReady() && complete.publishers==2 && complete.synchronizers==2 &&
        complete.requiredPairs==4 && complete.provedPairs==4 && complete.fingerprint && complete.classes.size()==4,"Ready prefix rejected");
    fixture.edges[0].erase({1,2});source=fixture.Compile();const auto missing=order::Analyze(source,Base,Read);
    Require(missing.failure==order::Failure::UnpublishedPrefix && missing.missing.size()==2 &&
        missing.missing[0].producer==0 && missing.missing[0].consumer==3,"Missing context producer not diagnosed");
    fixture=Fixture{};source=fixture.Compile();
    corruptLeaf=true;Require(order::Analyze(source,Base,Read).failure==order::Failure::Classification,"Changed type leaf accepted");corruptLeaf=false;
    unreadable=true;Require(order::Analyze(source,Base,Read).failure==order::Failure::Classification,"Unreadable type leaf accepted");unreadable=false;
    auto changed=source;Put(changed.nodes[0].definition,0,uint64_t(0));
    Require(order::Analyze(changed,Base,Read).failure==order::Failure::Classification,"Definition vtable mismatch accepted");
    changed=source;changed.nodes[0].work+=1;Require(order::Analyze(changed,Base,Read).failure==order::Failure::Classification,"Unknown work accepted");
    changed=source;Put(changed.nodes[1].definition,24,uint32_t(3));
    Require(order::Analyze(changed,Base,Read).failure==order::Failure::Classification,"Unknown synchronize mode accepted");
    changed=source;changed.nodes[0].bytes[29]=changed.nodes[1].bytes[29];
    Require(order::Analyze(changed,Base,Read).failure==order::Failure::Ranks,"Duplicate native slot accepted");
    changed=source;changed.nodes[0].order=1;Require(order::Analyze(changed,Base,Read).failure==order::Failure::Source,"Changed node ordinal accepted");
    changed=source;Put(changed.edges[0].bytes,4,uint32_t(2));Require(order::Analyze(changed,Base,Read).failure==order::Failure::Edges,"Unknown edge phase accepted");
    changed=source;Put(changed.edges[0].bytes,12,uint32_t(99));Require(order::Analyze(changed,Base,Read).failure==order::Failure::Edges,"Unknown endpoint accepted");
    constexpr uint64_t relocation=0x200000000;imageBase=Base+relocation;
    for(auto& node:source.nodes){node.vtable+=relocation;node.work+=relocation;Put(node.definition,0,node.vtable);}
    Require(order::Analyze(source,imageBase,Read).PrefixReady(),"Relocated image classification failed");
    if(argc==4)Saved(argv[1],argv[2],std::stoull(argv[3],nullptr,0));
    std::puts("PASS native execution context publication prefix and coupled CPU/order readiness");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
