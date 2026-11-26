#include "Render/PairedCpuFrontier.hpp"
#include <json/json.h>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <initializer_list>
#include <stdexcept>

namespace graph=cvr::stereo::graph_snapshot;
namespace frontier=cvr::stereo::paired_frontier;
namespace cpu=cvr::stereo::paired_cpu_frontier;
using frontier::Edges;
using frontier::Field;
constexpr uint64_t Base=0x140000000;
namespace {
void Require(bool value,const char* message){if(!value)throw std::runtime_error(message);}
template<class T,size_t N>void Put(std::array<uint8_t,N>& bytes,size_t offset,T value){std::memcpy(bytes.data()+offset,&value,sizeof(value));}
struct Fixture {
    graph::Snapshot graph;
    std::array<frontier::Roles,2> roles;
    std::array<Edges,2> edges;
    uint32_t global{};
    uint32_t Node(uint32_t eye,uint32_t work,uint64_t tag=0){
        const auto id=uint32_t(graph.nodes.size());graph::Node node;node.handle=node.order=id;
        Put(node.bytes,0,id);Put(node.bytes,8,uint32_t(4));Put(node.bytes,24,eye);Put(node.bytes,28,id|(id<<8));
        if(work){Put(node.bytes,16,uint64_t(0x100000+id*64));node.vtable=Base+0x2800000+id*16;
            node.work=Base+work;Put(node.definition,0,node.vtable);Put(node.definition,24,tag);}
        graph.nodes.push_back(node);return id;
    }
    uint32_t Named(uint32_t eye,std::string_view name){return Node(eye,0x332D5C,frontier::Name(name));}
    Fixture(){
        Named(0,"StartRender");global=Node(1,0x374AD8);
        for(uint32_t eye=0;eye<2;++eye){auto& role=roles[eye];role.collector=Node(eye,0xC0F204);
            for(const uint32_t kind:{17u,2u,4u})Node(eye,0xBF436C,kind);
            for(uint32_t particle=0;particle<4;++particle)Node(eye,0xD5BAC4,uint64_t(4)<<32|particle);
            Node(eye,0x77596C);Named(eye,"RenderPrep");Named(eye,"PrepareGloballyAccessibleSystems");Node(eye,0x655FAC,1);
            Named(eye,"AsyncWater");role.depth=Named(eye,"DepthPrepass");Named(eye,"GBufferEarly_Velocity_Weapon");Node(eye,0xCB1E88);
            for(uint32_t solid=0;solid<4;++solid)role.solid.push_back(Named(eye,"GBuffer_Solid"));
            role.solidReady=Node(eye,0x655FAC,2);role.discard=Named(eye,"GBuffer_Discard");
            role.afterDiscard=role.postGBuffer=Named(eye,"PostGBufferOverrides");role.lighting=Named(eye,"Lighting");role.final2D=Named(eye,"RenderFinal2D");
        }
        for(auto& phase:edges)for(uint32_t id=1;id<graph.nodes.size();++id)phase.emplace(id-1,id);
    }
    std::array<uint32_t,2> SyntheticPrefix(bool foreignEnd=false){
        const auto a=Node(1,0),b=Node(1,0);edges[0].erase({roles[0].final2D,roles[1].collector});
        const auto late=foreignEnd?Node(0,0):roles[0].final2D;
        if(foreignEnd)edges[0].emplace(roles[0].final2D,late);
        edges[0].emplace(late,a);edges[0].emplace(a,b);edges[0].emplace(b,roles[1].collector);
        edges[0].emplace(global,a);return {a,b};
    }
    graph::Snapshot Source(){
        graph.edges.clear();
        for(uint32_t phase=0;phase<2;++phase)for(const auto& [from,to]:edges[phase]){
            graph::Edge edge;edge.handle=edge.order=uint32_t(graph.edges.size());
            Put(edge.bytes,0,edge.order);Put(edge.bytes,4,phase);Put(edge.bytes,8,from|0x20000000u);Put(edge.bytes,12,to|0x20000000u);
            Put(edge.bytes,16,0x80000000u);Put(edge.bytes,20,0x80000000u);graph.edges.push_back(edge);
        }
        return graph;
    }
};
frontier::Plan SourcePlan(const graph::Snapshot& source,uint64_t base=Base,frontier::Boundary boundary=frontier::Boundary::Solid){
    frontier::Plan result;
    Require(result.BuildCandidate(source,base,boundary)==frontier::Result::CandidateNeedsCpuAndResourceCompile,"Invalid pass-frontier fixture");return result;
}
void ExactEdits(const cpu::Plan& plan){
    for(size_t phase=0;phase<2;++phase){auto restored=plan.Original()[phase];
        for(const auto edge:plan.Removed()[phase])Require(restored.erase(edge)==1,"Cut not present in original graph");
        for(const auto edge:plan.Added()[phase])Require(restored.insert(edge).second,"Addition already present in original graph");
        Require(restored==plan.Proposed()[phase],"Cut/add diagnostics do not reconstruct candidate");
    }
}
void ClosedWitness(const std::vector<uint32_t>& path,const Edges& edges){
    Require(path.size()>2 && path.front()==path.back(),"Cycle diagnostic omitted closed witness");
    for(size_t i=1;i<path.size();++i)Require(edges.contains({path[i-1],path[i]}),"Witness edge absent from diagnosed graph");
}
cpu::Plan Positive(const graph::Snapshot& source,uint64_t base=Base,frontier::Boundary boundary=frontier::Boundary::Solid){
    const auto original=source;const auto first=SourcePlan(source,base,boundary);cpu::Plan plan;
    Require(plan.BuildCandidate(first)==cpu::Result::CandidateNeedsOwnershipAndResourceCompile && plan.Rejection()==cpu::Failure::None,
        "Valid CPU topology rejected");
    Require(!plan.ExecutionReady() && plan.Matches(source,base) && !plan.Matches(source,base+1),"Topology became execution permission or lost identity");
    Require(plan.Source() && plan.Source()->nodes==original.nodes && plan.Source()->edges==original.edges &&
        source.nodes==original.nodes && source.edges==original.edges && plan.SourceFingerprint()==first.SourceFingerprint(),"Source mutated or fingerprint differs");
    const auto& p=plan.Proofs();Require(p.originalCpuAcyclic && p.originalOrderAcyclic && p.originalLiftedAcyclic &&
        p.candidateCpuAcyclic && p.candidateOrderAcyclic && p.candidateLiftedAcyclic &&
        p.boundedSyntheticAncestors && p.unmovedCpuEdgesPreserved && p.barriersPropagated && p.producersBeforeMainDiscard &&
        p.noMainPostDiscardToPeerReady && !p.nativeOwnershipProved && !p.resourceOrderCompiled && !p.nativeWaitReceiptsProved &&
        !p.executionPermitted,"Topology proof flags differ");
    Require(plan.WitnessPath().empty() && plan.Proposed()[1]==first.Proposed()[1],"Order plan changed or spurious witness");
    if(boundary==frontier::Boundary::Discard)Require(plan.Proofs().sharedOwnerOrderPreserved,"Shared object ordering was not preserved");
    const auto& main=first.EyeRoles()[0];const auto& peer=first.EyeRoles()[1];
    Require(plan.SelectedBoundary()==boundary && plan.EarlyTail()==first.EarlyTail(),"CPU boundary identity differs");
    for(const auto id:peer.prefix)Require(plan.Early().contains(id),"PEER prefix producer missing");
    Require(plan.Proposed()[0].contains({main.solidReady,peer.collector}) && plan.Proposed()[0].contains({first.EarlyTail(),main.discard}),"Required CPU ready links missing");
    if(boundary==frontier::Boundary::Discard)Require(plan.Early().contains(peer.discard) && !plan.Early().contains(main.discard) &&
        !plan.Early().contains(peer.afterDiscard) && plan.Proposed()[0].contains({peer.solidReady,peer.discard}) &&
        plan.Proposed()[1].contains({peer.solidReady,peer.discard}),"Whole discard closure or coupled CPU/ordered readiness edge lost");
    for(const auto& edge:plan.Original()[0])if(!plan.Early().contains(edge.first) && !plan.Early().contains(edge.second))
        Require(plan.Proposed()[0].contains(edge),"Original unmoved CPU edge lost");
    for(const auto barrier:plan.LateBarriers())for(const auto target:plan.FrontierTargets())
        if(boundary!=frontier::Boundary::Discard || target!=main.discard)
            Require(plan.Proposed()[0].contains({barrier,target}),"Late barrier did not reach every outgoing target");
    ExactEdits(plan);
    auto changed=source;changed.nodes[0].definition[16]^=1;Require(!plan.Matches(changed,base),"Definition mutation missed");
    changed=source;changed.edges[0].bytes[16]^=1;Require(!plan.Matches(changed,base),"Raw edge mutation missed");
    changed=source;changed.failure=1;Require(!plan.Matches(changed,base),"Failed source matched");
    return plan;
}
void SyntheticAndBarrierTests(){
    Fixture plain;auto p=Positive(plain.Source());Require(p.Early().size()==21 && p.SyntheticAncestors().empty(),"Basic early closure differs");
    Fixture joined;const auto joins=joined.SyntheticPrefix();p=Positive(joined.Source());
    Require(p.Early().size()==23 && p.SyntheticAncestors()==std::set<uint32_t>{joins[0],joins[1]},"Transitive synthetic ancestor closure incomplete");
    Require(!p.Early().contains(joined.global) && p.Proposed()[0].contains({joined.global,joins[0]}),"Already-early global real producer was moved");
    Require(p.LateBarriers()==std::set<uint32_t>{joined.roles[0].final2D} &&
        p.Removed()[0]==Edges{{joined.roles[0].final2D,joins[0]}},"Unexpected direct late cut");
    Fixture foreign;const auto foreignJoins=foreign.SyntheticPrefix(true);const auto late=uint32_t(foreign.graph.nodes.size()-1);
    p=Positive(foreign.Source());Require(!p.Early().contains(late) && p.LateBarriers().contains(late) &&
        p.Removed()[0].contains({late,foreignJoins[0]}) && p.Proposed()[0].contains({foreign.roles[0].final2D,late}),"CPU-only MAIN end join moved instead of retaining barrier");
    Fixture branching;const auto branchJoins=branching.SyntheticPrefix();
    branching.edges[0].emplace(branchJoins[0],branching.roles[1].lighting);branching.edges[0].emplace(branching.roles[0].discard,branchJoins[1]);
    p=Positive(branching.Source());Require(p.LateBarriers().size()==2 && p.FrontierTargets().contains(branching.roles[1].lighting),"Multiple barriers/frontier targets lost");
    Require(!p.LateBarriers().contains(branching.roles[0].solidReady),"MAIN ready was classified as late barrier");
    const auto held=p.Source();const auto fingerprint=p.SourceFingerprint();p=cpu::Plan{};
    Require(held && fingerprint==frontier::detail::Fingerprint(*held,Base),"Immutable diagnostic source expired or changed");
}
void Rejections(){
    cpu::Plan plan;frontier::Plan empty;
    Require(plan.BuildCandidate(empty)==cpu::Result::Rejected && plan.Rejection()==cpu::Failure::Source && !plan.Source(),"Invalid source plan accepted");
    for(unsigned bad=0;bad<5;++bad){Fixture fixture;const auto joins=fixture.SyntheticPrefix();auto& node=fixture.graph.nodes[joins[0]];
        switch(bad){case 0:Put(node.bytes,8,uint32_t(2));break;case 1:Put(node.bytes,16,uint64_t(0x123456));break;
        case 2:node.vtable=0x123456;break;case 3:node.definition[8]=1;break;default:Put(node.bytes,12,uint32_t(1));break;}
        const auto source=fixture.Source();const auto base=SourcePlan(source);
        Require(plan.BuildCandidate(base)==cpu::Result::Rejected && plan.Rejection()==cpu::Failure::SyntheticOwnership &&
            plan.Matches(source,Base) && !plan.WitnessPath().empty() && !plan.ExecutionReady(),"Unproved synthetic ownership accepted");
    }
    Fixture unproved;const auto joins=unproved.SyntheticPrefix();const auto foreign=unproved.Node(1,0x123456);
    unproved.edges[0].emplace(foreign,joins[0]);const auto unknownSource=unproved.Source();const auto unknown=SourcePlan(unknownSource);
    Require(plan.BuildCandidate(unknown)==cpu::Result::Rejected && plan.Rejection()==cpu::Failure::UnprovedPrerequisite &&
        !plan.Early().contains(foreign) && plan.WitnessPath()==std::vector<uint32_t>{foreign,joins[0]},"Unknown real CPU prerequisite absorbed");
    // Separate CPU and ordered-emission DAGs can legitimately have a cyclic raw union.
    Fixture crossed;const auto first=crossed.roles[1].collector+1,second=first+1;
    crossed.edges[0].erase({first,second});crossed.edges[0].emplace(crossed.roles[1].collector,second);crossed.edges[0].emplace(second,first);
    const auto crossSource=crossed.Source();const auto cross=SourcePlan(crossSource);plan=Positive(crossSource);
    Require(!plan.Proofs().originalRawUnionAcyclic && !plan.Proofs().candidateRawUnionAcyclic,"Cross-phase cycle diagnostic lost");
    ClosedWitness(plan.OriginalRawCycleWitness(),cpu::detail::Union(cross.Original()));
    ClosedWitness(plan.CandidateRawCycleWitness(),cpu::detail::Union(plan.Proposed()));
    // The new CPU link must reject an actual CPU-only cycle, not hide it in the event model.
    Fixture cycle;cycle.edges[0].erase({cycle.roles[0].final2D,cycle.roles[1].collector});
    cycle.edges[0].emplace(cycle.roles[1].collector,cycle.roles[0].solidReady);
    const auto cycleSource=cycle.Source();const auto cyclePlan=SourcePlan(cycleSource);
    Require(plan.BuildCandidate(cyclePlan)==cpu::Result::Rejected && plan.Rejection()==cpu::Failure::CpuCycle &&
        !plan.ExecutionReady(),"Candidate CPU cycle admitted");
    ClosedWitness(plan.WitnessPath(),plan.Proposed()[0]);ExactEdits(plan);
}
void Remapped(frontier::Boundary boundary=frontier::Boundary::Solid){
    Fixture fixture;fixture.SyntheticPrefix(true);auto source=fixture.Source();const auto expected=Positive(source,Base,boundary);
    std::map<uint32_t,uint32_t> ids;
    for(const auto& node:source.nodes)ids[node.handle]=1000+(uint32_t(source.nodes.size())-node.handle)*11;
    for(auto& node:source.nodes)node.handle=ids.at(node.handle)|0x20000000u;
    for(auto& edge:source.edges){for(const size_t offset:{size_t(8),size_t(12)})Put(edge.bytes,offset,ids.at(Field<uint32_t>(edge.bytes,offset)&0xDFFFFFFFu)|0x20000000u);
        edge.handle=(edge.handle+4000)|0x40000000u;}
    const auto mapped=Positive(source,Base,boundary);
    Require(mapped.EarlyTail()==ids.at(expected.EarlyTail()),"Early tail depended on node IDs");
    for(const auto id:expected.Early())Require(mapped.Early().contains(ids.at(id)),"Early closure depended on synthetic IDs");
    for(const auto id:expected.LateBarriers())Require(mapped.LateBarriers().contains(ids.at(id)),"Late barrier depended on IDs");
    for(size_t phase=0;phase<2;++phase)for(const auto& [from,to]:expected.Proposed()[phase])
        Require(mapped.Proposed()[phase].contains({ids.at(from),ids.at(to)}),"Remapped topology differs");
    constexpr uint64_t relocation=0x100000000;
    for(auto& node:source.nodes)if(node.work){node.work+=relocation;node.vtable+=relocation;Put(node.definition,0,node.vtable);}
    Positive(source,Base+relocation,boundary);
}
void WholeDiscard(){
    constexpr auto boundary=frontier::Boundary::Discard;
    Fixture plain;auto plan=Positive(plain.Source(),Base,boundary);
    Require(plan.Early().size()==22 && plan.SyntheticAncestors().empty(),"Whole pass omitted its discard CPU work");
    Fixture independentDiscard;independentDiscard.edges[0].erase({independentDiscard.roles[1].solidReady,independentDiscard.roles[1].discard});
    plan=Positive(independentDiscard.Source(),Base,boundary);
    Require(plan.Added()[0].contains({independentDiscard.roles[1].solidReady,independentDiscard.roles[1].discard}) &&
        !plan.Original()[0].contains({independentDiscard.roles[1].solidReady,independentDiscard.roles[1].discard}),
        "Independent discard lost explicit CPU solid-prefix readiness");
    Fixture joined;const auto joins=joined.SyntheticPrefix();const auto tailJoin=joined.Node(1,0);
    joined.edges[0].emplace(joins[1],tailJoin);joined.edges[0].emplace(tailJoin,joined.roles[1].discard);
    plan=Positive(joined.Source(),Base,boundary);
    Require(plan.Early().size()==25 && plan.SyntheticAncestors()==std::set<uint32_t>{joins[0],joins[1],tailJoin},
        "Discard-specific synthetic producer ancestor not captured");
    const auto& main=joined.roles[0];const auto& peer=joined.roles[1];
    Require(frontier::detail::Path(plan.Proposed()[0],main.discard,peer.discard).empty() &&
        !frontier::detail::Path(plan.Proposed()[0],peer.discard,main.discard).empty(),"Whole pass retained a MAIN post-discard blocker");
    Fixture exclusion;const auto& exclusionMain=exclusion.roles[0];const auto& exclusionPeer=exclusion.roles[1];
    exclusion.edges[0].erase({exclusionPeer.collector,exclusionPeer.collector+1});
    exclusion.edges[0].emplace(exclusionPeer.solidReady,exclusionMain.discard);
    plan=Positive(exclusion.Source(),Base,boundary);
    Require(plan.FrontierTargets().contains(exclusionMain.discard) && plan.LateBarriers().contains(exclusionMain.final2D) &&
        !plan.Added()[0].contains({exclusionMain.final2D,exclusionMain.discard}),"Late barrier propagated back into MAIN discard");
    Fixture foreign;const auto unknown=foreign.Node(1,0x123456);foreign.edges[0].emplace(unknown,foreign.roles[1].discard);
    const auto source=foreign.Source();const auto pass=SourcePlan(source,Base,boundary);
    Require(plan.BuildCandidate(pass)==cpu::Result::Rejected && plan.Rejection()==cpu::Failure::UnprovedPrerequisite &&
        plan.WitnessPath()==std::vector<uint32_t>{unknown,foreign.roles[1].discard},"Whole pass absorbed unknown real producer");
    Fixture unsafe;const auto tail=unsafe.Node(1,0);unsafe.graph.nodes[tail].definition[8]=1;
    unsafe.edges[0].emplace(tail,unsafe.roles[1].discard);
    Require(plan.BuildCandidate(SourcePlan(unsafe.Source(),Base,boundary))==cpu::Result::Rejected &&
        plan.Rejection()==cpu::Failure::SyntheticOwnership,"Whole pass accepted unaudited synthetic ancestor");
    Fixture cycle;cycle.edges[0].erase({cycle.roles[0].final2D,cycle.roles[1].collector});
    cycle.edges[0].emplace(cycle.roles[1].discard,cycle.roles[0].solidReady);
    Require(plan.BuildCandidate(SourcePlan(cycle.Source(),Base,boundary))==cpu::Result::Rejected &&
        plan.Rejection()==cpu::Failure::CpuCycle && !plan.ExecutionReady(),"Whole pass cycle became executable");
    ClosedWitness(plan.WitnessPath(),plan.Proposed()[0]);ExactEdits(plan);
    Remapped(boundary);
}
void SharedOwnerOrders(){
    constexpr auto boundary=frontier::Boundary::Discard;
    Fixture fixture;
    const auto main=fixture.roles[0].collector+3,peer=fixture.roles[1].collector+3;
    fixture.edges[0].erase({main,main+1});fixture.edges[0].emplace(main-1,main+1);
    fixture.edges[0].emplace(main,fixture.roles[0].final2D);
    auto& a=fixture.graph.nodes[main];auto& b=fixture.graph.nodes[peer];
    Put(b.bytes,16,Field<uint64_t>(a.bytes,16));b.vtable=a.vtable;b.work=a.work;b.definition=a.definition;
    const auto source=fixture.Source();const auto plan=Positive(source,Base,boundary);
    Require(plan.SharedOwnerOrders()==Edges{{main,peer}} && plan.RestoredOwnerOrders()==Edges{{main,peer}} &&
        plan.ReversedOwnerOrders().empty() && plan.Proposed()[0].contains({main,peer}),"Lost shared object CPU order was not restored");
    auto changed=source;changed.nodes[peer].vtable+=16;Put(changed.nodes[peer].definition,0,changed.nodes[peer].vtable);
    cpu::Plan rejected;Require(rejected.BuildCandidate(SourcePlan(changed,Base,boundary))==cpu::Result::Rejected &&
        rejected.Rejection()==cpu::Failure::SharedOwner && !rejected.ExecutionReady(),"One shared object acquired conflicting native types");
    Fixture reversed;auto& left=reversed.graph.nodes[reversed.roles[0].discard];auto& right=reversed.graph.nodes[reversed.roles[1].discard];
    Put(right.bytes,16,Field<uint64_t>(left.bytes,16));right.vtable=left.vtable;right.work=left.work;right.definition=left.definition;
    const auto whole=Positive(reversed.Source(),Base,boundary);
    Require(whole.ReversedOwnerOrders()==Edges{{reversed.roles[0].discard,reversed.roles[1].discard}} &&
        whole.RestoredOwnerOrders().empty(),"Intentional discard order was reverted instead of remaining serialized");
}
void LiftedEvents(){
    const std::set<uint32_t> nodes{10,20};
    const std::array<Edges,2> phases{Edges{{10,20}},Edges{{20,10}}};
    const auto lifted=cpu::detail::Lifted(nodes,phases);
    Require(lifted==Edges{{20,21},{40,41},{20,40},{41,21}},"Lifted model mixed CPU/order dependencies or omitted local ready edges");
    const std::set<uint32_t> events{20,21,40,41};
    Require(frontier::detail::Acyclic(events,lifted) && !cpu::detail::Cycle(nodes,cpu::detail::Union(phases)).empty(),
        "Cross-phase cycle was confused with one-way lifted event cycle");
    for(const auto& [from,to]:lifted)Require(!(from&1) || (to&1),"Unproved reverse native wait inserted");
    auto nativeWait=lifted;nativeWait.emplace(21,20);
    Require(!frontier::detail::Acyclic(events,nativeWait),"Example reverse native wait did not show missing execution proof");
}
template<class T>std::vector<T> ReadBinary(const std::filesystem::path& path){
    std::ifstream stream(path,std::ios::binary|std::ios::ate);Require(bool(stream),"Saved graph file missing");const auto bytes=stream.tellg();
    Require(bytes>=0 && uint64_t(bytes)%sizeof(T)==0 && uint64_t(bytes)<=64*1024*1024,"Saved graph ABI/size invalid");
    std::vector<T> values(static_cast<size_t>(bytes)/sizeof(T));stream.seekg(0);
    if(bytes)stream.read(reinterpret_cast<char*>(values.data()),bytes);Require(bool(stream),"Incomplete saved graph file");return values;
}
void SavedCapture(const std::filesystem::path& directory){
    Json::Value report;Json::CharReaderBuilder builder;std::string errors;
    std::ifstream input(directory/"preparation-graph.json");Require(bool(input) && Json::parseFromStream(builder,input,&report,&errors),"Saved graph JSON invalid");
    Require(report["meta"].isArray() && report["meta"].size()==4 && report["meta"][1].isUInt64(),"Saved image base metadata invalid");
    const auto base=report["meta"][1].asUInt64();graph::Snapshot source;
    source.nodes=ReadBinary<graph::Node>(directory/"preparation-graph-nodes.bin");source.edges=ReadBinary<graph::Edge>(directory/"preparation-graph-edges.bin");
    Require(source.nodes.size()==155 && source.edges.size()==928 && report["nodes"].size()==source.nodes.size() &&
        report["edges"].size()==source.edges.size(),"Saved285 graph dimensions differ");
    const auto pass=SourcePlan(source,base);const auto plan=Positive(source,base);
    Require(plan.Matches(source,base) && !plan.ExecutionReady(),"Saved source binding or nonexecution boundary lost");
    Require(plan.Early().size()==33 && plan.SyntheticAncestors().size()==12 && plan.LateBarriers().size()==32 &&
        plan.FrontierTargets().size()==35 && plan.Removed()[0].size()==32 && plan.Added()[0].size()==1090 &&
        plan.LateBarriers().contains(82) && !plan.Early().contains(82) && !plan.Early().contains(84),
        "Saved285 bounded CPU closure/cuts differ");
    Require(plan.Proposed()[0].contains({57,82}) && !plan.Proposed()[0].contains({82,88}) &&
        plan.Proposed()[0].contains({84,155}),"Saved285 shared/global dependency handling differs");
    Require(!plan.Proofs().originalRawUnionAcyclic,"Saved285 cross-phase cycle diagnostic lost");
    ClosedWitness(plan.OriginalRawCycleWitness(),cpu::detail::Union(pass.Original()));
    const auto wholePass=SourcePlan(source,base,frontier::Boundary::Discard);const auto whole=Positive(source,base,frontier::Boundary::Discard);
    const auto& main=wholePass.EyeRoles()[0];const auto& peer=wholePass.EyeRoles()[1];
    Require(whole.EarlyTail()==peer.discard && peer.afterDiscard==110 && whole.Early().size()==34 &&
        whole.SyntheticAncestors().size()==12 && whole.LateBarriers().size()==32 && whole.FrontierTargets().size()==34 &&
        whole.Removed()[0].size()==32 && whole.Added()[0].size()==1060 && whole.Proposed()[0].size()==1837 &&
        whole.Proposed()[1].size()==119,"Saved whole-discard closure/cuts differ");
    std::printf("Shared owner orders %zu, restored %zu, reversed %zu\n",whole.SharedOwnerOrders().size(),
        whole.RestoredOwnerOrders().size(),whole.ReversedOwnerOrders().size());
    Require(whole.RestoredOwnerOrders()==Edges{{8,91}} && whole.ReversedOwnerOrders()==Edges{{26,109}} &&
        whole.SharedOwnerOrders().size()==52 && whole.Proofs().sharedOwnerOrderPreserved,
        "Saved shared-outer-object CPU contract differs");
    Require(!whole.Original()[0].contains({peer.solidReady,peer.discard}) && whole.Proposed()[0].contains({peer.solidReady,peer.discard}) &&
        whole.Proposed()[0].contains({main.solidReady,peer.collector}) && whole.Proposed()[0].contains({peer.discard,main.discard}) &&
        whole.Proposed()[1].contains({peer.solidReady,peer.discard}) && whole.Proposed()[1].contains({peer.discard,main.discard}) &&
        whole.Proposed()[1].contains({main.final2D,peer.afterDiscard}),"Saved whole-pass boundary links differ");
    Require(wholePass.CpuBlockerPath().front()==main.discard && wholePass.CpuBlockerPath().back()==peer.discard &&
        wholePass.OrderBlockerPath().front()==main.discard && wholePass.OrderBlockerPath().back()==peer.discard &&
        frontier::detail::Path(whole.Proposed()[0],main.discard,peer.discard).empty(),"Saved whole-pass blocker diagnostics differ");
    std::printf("Saved whole-discard topology: early %zu, CPU %zu, order %zu; native mode2 context/lifetime/physical-alias proofs remain pending\n",
        whole.Early().size(),whole.Proposed()[0].size(),whole.Proposed()[1].size());
    std::printf("Saved285 non-executable topology source %016llx: early %zu, late %zu, cut %zu, add %zu; original raw-union witness:",
        static_cast<unsigned long long>(plan.SourceFingerprint()),plan.Early().size(),plan.LateBarriers().size(),
        plan.Removed()[0].size(),plan.Added()[0].size());
    for(const auto id:plan.OriginalRawCycleWitness())std::printf(" %u",id);std::puts("");
}
}
int main(int argc,char** argv)try{
    Require(argc==1 || argc==2,"paired_cpu_frontier_test [saved graph capture directory]");
    SyntheticAndBarrierTests();Rejections();Remapped();LiftedEvents();WholeDiscard();SharedOwnerOrders();if(argc==2)SavedCapture(std::filesystem::path(argv[1]));
    std::puts("PASS source-bound solid/discard CPU frontiers, synthetic closure, barrier propagation, remapped IDs and cycle witnesses");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
