#pragma once
#include "Render/SceneGraphExpansion.hpp"
#include <stdexcept>

namespace cvr::stereo::scene_expansion {
// Compose two source-bound expansions without moving either eye's tail before
// the other eye's prefix. This does not admit execution or suppress native work.
class BothEyesPlan {
public:
#if CVR_RUST_VRIK
    // The rules: rust/crates/cvr-render/src/scene_expansion.rs (tools/single_pass_tests/both_eyes_scene_expansion_test runs
    // its cases against them in its Rust variant); the two plans stay members here (each Rust's).
    bool Build(const graph_snapshot::Snapshot& source,uint64_t base,
               const std::array<std::array<Definition,2>,2>& definitions)try{
        *this={};const auto lent=detail::Lend(source);std::array<graph_snapshot::Node,2> selected{};
        failure_=Failure(detail::cvr_sxb_select(&lent,base,&definitions,&selected));if(failure_!=Failure::None)return false;
        // Capture both original nodes before the first expansion changes ranks
        // and adjacency in the second plan's source.
        selected_=selected;
        if(!peer_.Build(source,base,selected_[1].handle,definitions[1])){failure_=peer_.Rejection();return false;}
        if(!main_.Build(peer_.Expanded(),base,selected_[0].handle,definitions[0])){failure_=main_.Rejection();return false;}
        const auto peer=peer_.Frozen(),main=main_.Frozen();
        const auto cpu=paired_frontier::detail::Flat(main_.Edges()[0]),order=paired_frontier::detail::Flat(main_.Edges()[1]);
        detail::RustMappings* held{};const Mapping* composed{};size_t count{};
        failure_=Failure(detail::cvr_sxb_compose(&lent,&selected_,&peer,&main,cpu.data(),cpu.size(),order.data(),order.size(),
            &held,&composed,&count));
        struct Release{detail::RustMappings* held;~Release(){detail::cvr_sxb_free(held);}}release{held};
        mappings_.assign(composed,composed+count);
        if(failure_!=Failure::None)return false;
        base_=base;ready_=true;return true;
    }catch(...){*this={};failure_=Failure::Allocation;return false;}
#else
    bool Build(const graph_snapshot::Snapshot& source,uint64_t base,
               const std::array<std::array<Definition,2>,2>& definitions)try{
        *this={};failure_=Failure::Source;
        if(!base || base>UINT64_MAX-0x3094850 || source.failure || source.nodes.empty())return false;
        failure_=Failure::Capacity;
        if(source.nodes.size()>218)return false;
        failure_=Failure::Identity;
        std::array<const graph_snapshot::Node*,2> selected{};
        for(const auto& node:source.nodes){
            if(graph_snapshot::Field<uint64_t>(node.definition,24)!=paired_frontier::Name("GBuffer_Discard"))continue;
            const auto view=graph_snapshot::Field<uint32_t>(node.bytes,24);
            if(view>1 || selected[view])return false;
            selected[view]=&node;
        }
        if(!selected[0] || !selected[1])return false;
        if(graph_snapshot::Field<uint64_t>(selected[0]->bytes,16)==graph_snapshot::Field<uint64_t>(selected[1]->bytes,16) &&
            selected[0]->definition!=selected[1]->definition)return false;
        failure_=Failure::Definitions;
        for(uint32_t view=0;view<2;++view)for(uint32_t section=0;section<2;++section){
            const auto& definition=definitions[view][section];
            if(!definition.object || (definition.object&7) || definition.object>UINT64_MAX-56 ||
                definition.prefix!=selected[view]->definition)return false;
            for(const auto& node:source.nodes){
                const auto object=graph_snapshot::Field<uint64_t>(node.bytes,16);
                const uint64_t bytes=node.vtable==base+0x3094850 && node.work==base+0x332D5C?56:32;
                if(object && Overlap(definition.object,56,object,bytes))return false;
            }
            for(uint32_t other=0;other<view*2+section;++other)
                if(Overlap(definition.object,56,definitions[other/2][other%2].object,56))return false;
        }
        // Capture both original nodes before the first expansion changes ranks
        // and adjacency in the second plan's source.
        selected_[0]=*selected[0];selected_[1]=*selected[1];
        if(!peer_.Build(source,base,selected_[1].handle,definitions[1])){failure_=peer_.Rejection();return false;}
        if(!main_.Build(peer_.Expanded(),base,selected_[0].handle,definitions[0])){failure_=main_.Rejection();return false;}
        failure_=Failure::Contraction;
        for(const auto& second:main_.Mappings()){
            const auto first=std::find_if(peer_.Mappings().begin(),peer_.Mappings().end(),[&](const auto& mapping){
                return mapping.executionHandle==second.sourceHandle;
            });
            if(first==peer_.Mappings().end() || (first->section && second.section))return false;
            mappings_.push_back({first->sourceHandle,second.executionHandle,second.section?second.section:first->section});
        }
        if(mappings_.size()!=source.nodes.size()+2 || !ContractedEdgesMatch(source))return false;
        failure_=Failure::Topology;
        for(uint32_t phase=0;phase<2;++phase){
            const auto& expanded=main_.Expanded();
            const auto* peerPrefix=Find(expanded,peer_.PrefixHandle());
            const auto* peerTail=Find(expanded,peer_.TailHandle());
            const auto* mainPrefix=Find(expanded,main_.PrefixHandle());
            const auto* mainTail=Find(expanded,main_.TailHandle());
            if(!peerPrefix || !peerTail || !mainPrefix || !mainTail ||
                selected_[1].bytes[28+phase]>=selected_[0].bytes[28+phase] ||
                peerPrefix->bytes[28+phase]>=peerTail->bytes[28+phase] ||
                peerTail->bytes[28+phase]>=mainPrefix->bytes[28+phase] ||
                mainPrefix->bytes[28+phase]>=mainTail->bytes[28+phase])return false;
            const auto& edges=main_.Edges()[phase];
            const std::array<uint32_t,4> chain{peer_.PrefixHandle(),peer_.TailHandle(),main_.PrefixHandle(),main_.TailHandle()};
            for(size_t i=1;i<chain.size();++i)
                if(paired_frontier::detail::Path(edges,chain[i-1]&0xDFFFFFFFu,chain[i]&0xDFFFFFFFu).empty())return false;
        }
        base_=base;ready_=true;failure_=Failure::None;return true;
    }catch(...){*this={};failure_=Failure::Allocation;return false;}
#endif
    bool MatchesSource(const graph_snapshot::Snapshot& source,uint64_t base)const{
        return ready_ && peer_.MatchesSource(source,base);
    }
    bool MatchesCompiled(const graph_snapshot::Snapshot& actual)const{
        return ready_ && main_.MatchesCompiled(actual);
    }
#if CVR_RUST_VRIK
    bool MatchesDeclaration(const graph_snapshot::Snapshot& source,const graph_snapshot::Snapshot& expanded,uint64_t base)const{
        if(!ready_)return false;
        const auto peer=peer_.Frozen(),main=main_.Frozen();const auto current=detail::Lend(source),compiled=detail::Lend(expanded);
        return detail::cvr_sxb_matches_declaration(&peer,&main,base_,&current,&compiled,base);
    }
#else
    bool MatchesDeclaration(const graph_snapshot::Snapshot& source,const graph_snapshot::Snapshot& expanded,uint64_t base)const{
        if(!ready_ || base!=base_ || source.failure || source.nodes.size()!=Source().nodes.size())return false;
        auto intermediate=peer_.Expanded();
        if(intermediate.nodes.size()!=peer_.Mappings().size())return false;
        for(size_t i=0;i<intermediate.nodes.size();++i){
            const auto& mapping=peer_.Mappings()[i];
            if(mapping.section)continue;
            const auto* original=Find(source,mapping.sourceHandle);
            if(!original)return false;
            intermediate.nodes[i].definition=original->definition;
        }
        return peer_.MatchesDeclaration(source,intermediate,base) && main_.MatchesDeclaration(intermediate,expanded,base);
    }
#endif
    bool Ready()const{return ready_;}
    Failure Rejection()const{return failure_;}
    const graph_snapshot::Snapshot& Source()const{return peer_.Source();}
    const graph_snapshot::Snapshot& Expanded()const{return main_.Expanded();}
    const std::array<paired_frontier::Edges,2>& Edges()const{return main_.Edges();}
    const std::vector<Mapping>& Mappings()const{return mappings_;}
    const graph_snapshot::Node& SelectedSource(uint32_t view)const{return selected_.at(view);}
    const std::array<Definition,2>& Definitions(uint32_t view)const{return View(view).Definitions();}
    uint32_t PrefixHandle(uint32_t view)const{return View(view).PrefixHandle();}
    uint32_t TailHandle(uint32_t view)const{return View(view).TailHandle();}
    bool ExecutionPermitted()const{return false;}
private:
    const Plan& View(uint32_t view)const{
        if(view>1)throw std::out_of_range("Native eye index");
        return view?peer_:main_;
    }
#if !CVR_RUST_VRIK
    static const graph_snapshot::Node* Find(const graph_snapshot::Snapshot& source,uint32_t handle){
        const auto found=std::find_if(source.nodes.begin(),source.nodes.end(),[&](const auto& node){return node.handle==handle;});
        return found==source.nodes.end()?nullptr:&*found;
    }
    static bool Overlap(uint64_t a,uint64_t aBytes,uint64_t b,uint64_t bBytes){
        return a>UINT64_MAX-aBytes || b>UINT64_MAX-bBytes || (a<b+bBytes && b<a+aBytes);
    }
    bool ContractedEdgesMatch(const graph_snapshot::Snapshot& source)const{
        std::map<uint32_t,uint32_t> inverse;
        for(const auto& mapping:mappings_)
            if(!inverse.emplace(mapping.executionHandle&0xDFFFFFFFu,mapping.sourceHandle&0xDFFFFFFFu).second)return false;
        std::array<paired_frontier::Edges,2> original;
        for(const auto& edge:source.edges){
            const auto phase=graph_snapshot::Field<uint32_t>(edge.bytes,4);
            if(phase>1 || !original[phase].emplace(graph_snapshot::Field<uint32_t>(edge.bytes,8)&0xDFFFFFFFu,
                graph_snapshot::Field<uint32_t>(edge.bytes,12)&0xDFFFFFFFu).second)return false;
        }
        for(uint32_t phase=0;phase<2;++phase){
            paired_frontier::Edges contracted;uint32_t internal{};
            for(const auto& [from,to]:main_.Edges()[phase]){
                if((from==(peer_.PrefixHandle()&0xDFFFFFFFu) && to==(peer_.TailHandle()&0xDFFFFFFFu)) ||
                    (from==(main_.PrefixHandle()&0xDFFFFFFFu) && to==(main_.TailHandle()&0xDFFFFFFFu))){++internal;continue;}
                const auto a=inverse.find(from),b=inverse.find(to);
                if(a==inverse.end() || b==inverse.end() || a->second==b->second ||
                    !contracted.emplace(a->second,b->second).second)return false;
            }
            if(internal!=2 || contracted!=original[phase])return false;
        }
        return true;
    }
#endif
    bool ready_{};Failure failure_=Failure::None;uint64_t base_{};
    Plan peer_,main_;
    std::array<graph_snapshot::Node,2> selected_{};
    std::vector<Mapping> mappings_;
};
}
