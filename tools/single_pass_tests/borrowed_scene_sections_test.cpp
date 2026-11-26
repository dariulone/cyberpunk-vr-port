#include "Render/BorrowedSceneSections.hpp"
#include <cstdio>
#include <map>
#include <source_location>
#include <stdexcept>
#include <string>
#include <type_traits>

namespace borrowed=cvr::stereo::borrowed_scene_sections;
namespace sections=cvr::stereo::scene_sections;
namespace tree=cvr::stereo::definition_tree;
namespace stage=cvr::stereo::stage_definition;
namespace graph=cvr::stereo::graph_snapshot;
namespace {
constexpr uint64_t Base=0x140000000,Root=0x10000,Handles=0x20000,Children=0x30000,Controls=0x40000;
constexpr uint64_t Early=0x50000,Late=0x51000,ArrayAllocator=0x62000;
std::map<uint64_t,uint8_t> memory;
void Require(bool value,std::source_location at=std::source_location::current()){
    if(!value)throw std::runtime_error("borrowed scene sections assertion at line "+std::to_string(at.line()));
}
template<class T>void Put(uint64_t address,const T& value){const auto* bytes=reinterpret_cast<const uint8_t*>(&value);
    for(size_t i=0;i<sizeof(value);++i)memory[address+i]=bytes[i];}
template<class T,size_t N>void Field(std::array<uint8_t,N>& bytes,size_t at,T value){std::memcpy(bytes.data()+at,&value,sizeof(value));}
bool Read(uint64_t address,void* output,size_t count){
    auto* bytes=static_cast<uint8_t*>(output);if(!address || address>UINT64_MAX-count)return false;
    for(size_t i=0;i<count;++i){const auto found=memory.find(address+i);if(found==memory.end())return false;bytes[i]=found->second;}
    return true;
}
constexpr uint64_t Object(uint32_t slot){return Children+uint64_t(slot)*0x100;}
constexpr uint64_t Control(uint32_t slot){return Controls+uint64_t(slot)*0x100;}
// Synthetic metadata only. No native calls or claimed extraReader authority.
void Fresh(){
    memory.clear();Put(Base+tree::ObjectAllocatorRva,uint64_t(0x60000));Put(Base+tree::ControlAllocatorRva,uint64_t(0x61000));
    Put(ArrayAllocator+0x20,Base+0x985ABC);
    Put(Base+sections::VtableRva,Base+tree::SceneDriverDestructorRva);Put(Base+sections::VtableRva+8,Base+sections::WorkRva);
    sections::Header root{};Field(root,0,Base+sections::VtableRva);Field(root,24,sections::Name("GBuffer_Discard"));
    Field(root,32,uint32_t(1));root[36]=0xB7;root[39]=0xA1;
    Field(root,40,Handles);Field(root,48,sections::ChildCount);Field(root,52,sections::ChildCount);Put(Root,root);
    Put(Handles+16*sections::ChildCount,ArrayAllocator);
    for(uint32_t slot=0;slot<sections::ChildCount;++slot){const auto& profile=sections::detail::Pattern[slot];
        const auto vtable=Base+profile.vtable;Put(vtable,Base+profile.destructor);Put(vtable+8,Base+profile.work);
        if(slot==2 || slot==8){const bool late=slot==8;const uint64_t data=late?Late:Early;const uint32_t count=late?4:2;
            stage::Header header{};Field(header,0,vtable);Field(header,24,Base+(late?0x22749E0:0x2274AE0));
            Field(header,48,data);Field(header,56,count);Field(header,60,count);Put(Object(slot),header);
            for(uint32_t i=0;i<count;++i){stage::Stage value{};Field(value,8,i%2?17u:33u);value[12]=uint8_t(i%2?1:0);
                Field(value,16,late?(i<2?9u:8u):7u);value[20]=uint8_t(i%2?0:2);Field(value,36,UINT32_MAX);
                value[21]=uint8_t(0x80+i);Put(data+48*i,value);
            }
            Put(data+48*count,ArrayAllocator);
        }else if(profile.bytes==24){std::array<uint8_t,24> child{};Field(child,0,vtable);child[9]=0x91;Put(Object(slot),child);
        }else{std::array<uint8_t,32> child{};Field(child,0,vtable);Field(child,24,profile.parameter);child[9]=0x91;Put(Object(slot),child);}
        Put(Handles+16*slot,tree::NativeHandle{Object(slot),Control(slot)});Put(Control(slot),int32_t(1));Put(Control(slot)+4,int32_t(1));
    }
    for(const auto pair:std::array<std::array<uint32_t,2>,4>{{{0,6},{1,7},{4,9},{5,10}}}){
        Put(Handles+16*pair[1],tree::NativeHandle{Object(pair[0]),Control(pair[0])});Put(Control(pair[0]),int32_t(2));
    }
}
graph::Node Graph(uint32_t view=0){graph::Node node;node.handle=0x2000001A+view;node.order=2;
    node.vtable=Base+sections::VtableRva;node.work=Base+sections::WorkRva;Field(node.bytes,0,node.order);
    Field(node.bytes,16,Root);Field(node.bytes,24,view);Require(Read(Root,node.definition.data(),node.definition.size()));return node;}
bool Capture(sections::Snapshot& source,uint32_t view=0){tree::Snapshot native;const uint64_t roots[]{Root};
    return native.Capture(roots,Base,Read,true) && source.Capture(Graph(view),native,Read);}
borrowed::CallerLifetime Binding(){return {11,12,13,Root,0x70000,14,15,16};}
sections::Header Header(const cvr::stereo::scene_expansion::Definition& definition){sections::Header bytes{};
    std::memcpy(bytes.data(),reinterpret_cast<const void*>(uintptr_t(definition.object)),bytes.size());return bytes;}

void PositiveAndMoves(){
    Fresh();sections::Snapshot source;Require(Capture(source));const auto before=memory;const auto binding=Binding();
    borrowed::Sections value;Require(value.Create(source,binding) && value.Ready() && value.Matches(source,binding));
    Require(!value.ExecutionPermission() && !value.NativeOwnership() && memory==before);
    const auto definitions=value.Definitions();Require(definitions[0].object!=definitions[1].object);
    for(size_t i=0;i<2;++i){const auto count=i?5u:6u,begin=i?6u:0u;const auto header=Header(definitions[i]);
        Require(definitions[i].object && !(definitions[i].object&7) && definitions[i].object!=Root);
        Require(std::equal(header.begin(),header.begin()+40,source.RawHeader().begin()) && definitions[i].prefix==source.RootIdentity().definition);
        Require(graph::Field<uint32_t>(header,48)==count && graph::Field<uint32_t>(header,52)==count);
        const auto data=graph::Field<uint64_t>(header,40);Require(data && !(data&7) && data!=Handles);
        const auto* handles=reinterpret_cast<const tree::NativeHandle*>(uintptr_t(data));
        Require(std::equal(handles,handles+count,source.Handles().begin()+begin));
    }
    Require(!value.Create(source,binding) && value.Rejection()==borrowed::Failure::AlreadyPrepared && value.Definitions()==definitions);
    borrowed::Sections moved(std::move(value));Require(!value.Ready() && value.Definitions()[0].object==0);
    Require(moved.Definitions()==definitions && moved.Matches(source,binding));
    borrowed::Sections assigned;assigned=std::move(moved);Require(!moved.Ready() && assigned.Definitions()==definitions && assigned.Matches(source,binding));
    Require(value.Prepare(source) && value.Prepared() && !value.Bound() && !value.Ready() && !value.Matches(source,binding));
    Require(memory==before);
}
void TwoPhase(){
    Fresh();sections::Snapshot source;Require(Capture(source));const auto binding=Binding();borrowed::Sections value;
    Require(!value.Bind(source,binding) && value.Rejection()==borrowed::Failure::NotPrepared && !value.Prepared());
    Require(value.Prepare(source) && value.Prepared() && !value.Ready() && !value.Bound() && !value.Matches(source,binding));
    const auto definitions=value.Definitions();Require(definitions[0].object && definitions[1].object);
    auto bad=binding;bad.protectionReceipt=0;
    Require(!value.Bind(source,bad) && value.Rejection()==borrowed::Failure::Binding && value.Prepared() && !value.Bound());
    Require(value.Definitions()==definitions && !value.Matches(source,binding));
    sections::Snapshot other;Require(Capture(other,1));
    Require(!value.Bind(other,binding) && value.Rejection()==borrowed::Failure::Changed && !value.Bound() && value.Definitions()==definitions);
    Require(value.Bind(source,binding) && value.Bound() && value.Ready() && value.Definitions()==definitions && value.Matches(source,binding));
    Require(!value.Bind(source,binding) && value.Rejection()==borrowed::Failure::AlreadyBound && value.Matches(source,binding));
    bad=binding;++bad.protectionReceipt;
    Require(!value.Bind(source,bad) && value.Rejection()==borrowed::Failure::AlreadyBound && !value.Matches(source,bad));
    Require(value.Definitions()==definitions && !value.ExecutionPermission() && !value.NativeOwnership());
}
void Rejections(){
    Fresh();sections::Snapshot source;Require(Capture(source));const auto binding=Binding();
    const std::array<uint64_t borrowed::CallerLifetime::*,8> fields{
        &borrowed::CallerLifetime::consumerId,&borrowed::CallerLifetime::epoch,&borrowed::CallerLifetime::pairGeneration,
        &borrowed::CallerLifetime::originalRoot,&borrowed::CallerLifetime::ownerHeader,&borrowed::CallerLifetime::ownerGeneration,
        &borrowed::CallerLifetime::ownerBornEpoch,&borrowed::CallerLifetime::protectionReceipt};
    for(const auto field:fields){auto bad=binding;bad.*field=0;borrowed::Sections value;
        Require(!value.Create(source,bad) && value.Prepared() && !value.Ready() && !value.Bound() &&
            value.Rejection()==borrowed::Failure::Binding && value.Definitions()[0].object);
    }
    for(const auto address:{uint64_t(1),UINT64_MAX-7}){auto bad=binding;bad.ownerHeader=address;borrowed::Sections value;Require(!value.Create(source,bad));}
    auto bad=binding;bad.originalRoot+=8;borrowed::Sections value;Require(!value.Create(source,bad));
    borrowed::Sections empty;Require(!empty.Create(sections::Snapshot{},binding) && !empty.Prepared() && !empty.Ready());
    Require(value.Bind(source,binding));for(const auto field:fields){bad=binding;++(bad.*field);Require(!value.Matches(source,bad));}
    sections::Snapshot other;Require(Capture(other,1) && !value.Matches(other,binding));
    Put(Root+52,uint32_t(10));sections::Snapshot shortSource;Require(!Capture(shortSource));borrowed::Sections rejected;
    Require(!rejected.Create(shortSource,binding) && !rejected.Ready());
}
void ChangedOriginal(){
    for(const auto address:{Root+36,Object(3)+9,Late+3*48+21}){
        Fresh();sections::Snapshot source;Require(Capture(source));borrowed::Sections value;const auto binding=Binding();Require(value.Create(source,binding));
        memory[address]^=1;Require(!source.MatchesBefore(Read));
        // Matches is deliberately not a replacement for the live-source check.
        Require(value.Matches(source,binding) && !value.ExecutionPermission());
        sections::Snapshot changed;Require(Capture(changed) && !value.Matches(changed,binding));
    }
}
void Contracts(){
    std::map<uint32_t,uint32_t> spans;
    for(const auto& code:borrowed::Contract){Require(code.rva && code.bytes && code.hash && spans.emplace(code.rva,code.bytes).second);}
    for(const auto rva:borrowed::VtableTargets)Require(spans.contains(rva));
    Require(spans.at(0x2457DD4)==13 && spans.at(0x252C20C)==22 && spans.at(0x24E89A6)==19 && spans.at(0x2442FCA)==62);
}
static_assert(!std::is_copy_constructible_v<borrowed::Sections> && !std::is_copy_assignable_v<borrowed::Sections>);
static_assert(std::is_nothrow_move_constructible_v<borrowed::Sections> && std::is_nothrow_move_assignable_v<borrowed::Sections>);
}
int main()try{PositiveAndMoves();TwoPhase();Rejections();ChangedOriginal();Contracts();
    std::puts("PASS immutable C++ scene-section storage with borrowed handles and separate caller lifetime identity");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
