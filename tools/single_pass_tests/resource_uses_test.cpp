#include "Render/NativeResourceUses.hpp"
#include <cstdio>
#include <climits>
#include <functional>
#include <map>
#include <stdexcept>
#include <utility>

using namespace cvr::stereo;
namespace {
void Require(bool good){if(!good)throw std::runtime_error("native resource uses assertion failed");}
constexpr uint64_t Pool=0x10000000;
struct Memory {
    std::map<uint64_t,std::vector<uint8_t>> values;
    std::map<uint64_t,size_t> calls;
    std::function<void(Memory&,uint64_t,size_t)> before;
    size_t reads{};
    template<class T>void Put(uint64_t address,const T& value){
        auto& data=values[address];data.resize(sizeof(value));std::memcpy(data.data(),&value,sizeof(value));
    }
    void Ids(std::initializer_list<uint64_t> ids){
        auto& data=values[Pool+resource_uses::UseIds];data.resize(ids.size()*8);
        if(data.size())std::memcpy(data.data(),ids.begin(),data.size());
        Put(Pool+resource_uses::UseCount,uint32_t(ids.size()));
    }
    static bool Read(void* user,uint64_t address,void* output,size_t bytes){
        auto& self=*static_cast<Memory*>(user);++self.reads;const auto call=++self.calls[address];
        if(self.before)self.before(self,address,call);
        auto it=self.values.upper_bound(address);if(it==self.values.begin())return false;--it;
        const auto offset=address-it->first;
        if(offset>it->second.size() || bytes>it->second.size()-offset)return false;
        std::memcpy(output,it->second.data()+offset,bytes);return true;
    }
};
template<class T,size_t N>void Set(std::array<uint8_t,N>& bytes,size_t offset,T value){std::memcpy(bytes.data()+offset,&value,sizeof(value));}
uint64_t Id(uint32_t slot,uint32_t index,uint32_t key){return (uint64_t(key)<<32)|(uint64_t(slot)<<16)|index;}
graph_snapshot::Snapshot Graph(uint32_t count=2){
    graph_snapshot::Snapshot result;
    for(uint32_t i=0;i<count;++i){graph_snapshot::Node node;node.handle=(100+i)|0x20000000;node.order=i;
        Set(node.bytes,0,i);node.bytes[0x1d]=uint8_t(i);result.nodes.push_back(node);}
    return result;
}
Memory Fixture(){
    Memory memory;memory.Put(Pool,std::array<uint8_t,4>{0,0,1,0});
    memory.Put(Pool+resource_uses::AuxCount,uint32_t(0));memory.Put(Pool+resource_uses::BoundaryCount,uint32_t(0));
    memory.Put(Pool+resource_uses::DeclarationCount,uint32_t(2));
    memory.Put(Pool+resource_uses::SlotStride+resource_uses::DeclarationCount,uint32_t(1));
    std::array<uint8_t,48> first{},second{},third{};
    first[0]=4;Set(first,4,uint32_t(0x123));Set(first,8,uint32_t(3));first[47]=0x55;
    second[0]=5;Set(second,4,uint32_t(0x123));
    third[0]=7;third[1]=1;Set(third,4,uint32_t(0x456));Set(third,8,uint32_t(0x123));
    memory.Put(Pool+resource_uses::Declarations,first);
    memory.Put(Pool+resource_uses::Declarations+48,second);
    memory.Put(Pool+resource_uses::SlotStride+resource_uses::Declarations,third);
    memory.Ids({Id(1,0,0x456),Id(0,1,0x123),Id(0,0,0x123),Id(0,1,0x123)});
    return memory;
}
bool Capture(Memory& memory,resource_uses::Snapshot& out,const graph_snapshot::Snapshot& graph=Graph()){
    return out.CapturePool(Pool,graph,Memory::Read,&memory,0x9988);
}
void Rejected(Memory& memory,resource_uses::Failure reason,const graph_snapshot::Snapshot& graph=Graph()){
    resource_uses::Snapshot out;out.pool=1;out.useIds={7};out.declarations.push_back({});
    Require(!Capture(memory,out,graph) && out.failure==reason && !out.pool && !out.contextRoot && out.useIds.empty() && out.declarations.empty());
}
void Valid(){
    auto memory=Fixture();resource_uses::Snapshot out;Require(Capture(memory,out));
    Require(out.failure==resource_uses::Failure::None && out.pool==Pool && out.contextRoot==0x9988 && out.opaqueStateUnsupported);
    Require(out.useIds.size()==4 && out.declarations.size()==3 && out.useIds[1]==out.useIds[3]);
    Require(out.declarations[0].slot==1 && out.declarations[0].nodeHandle==(101|0x20000000u) && out.declarations[0].nodeOrder==1);
    Require(out.declarations[2].bytes[47]==0x55 && out.kinds[4]==1 && out.kinds[5]==1 && out.kinds[7]==1);
    auto graph=Graph();Require(out.sourceNodes==graph.nodes);graph.nodes.clear();memory.values.clear();
    Require(out.sourceNodes.size()==2 && out.declarations[2].bytes[47]==0x55);
    memory=Fixture();memory.Ids({});Require(Capture(memory,out) && out.declarations.empty());
    memory=Fixture();Require(Capture(memory,out,Graph(155)) && out.sourceNodes.size()==155);
    memory=Fixture();auto& raw=memory.values.at(Pool+resource_uses::Declarations);raw[0]=250;
    Require(Capture(memory,out) && out.declarations[2].kind==250 && out.opaqueStateUnsupported);
}
void ArgumentsAndGraph(){
    auto memory=Fixture();resource_uses::Snapshot out;
    Require(!out.CapturePool(0,Graph(),Memory::Read,&memory));
    Require(!out.CapturePool(UINT64_MAX,Graph(),Memory::Read,&memory));
    Require(!out.CapturePool(Pool,Graph(),nullptr,&memory));Require(memory.reads==0);
    for(uint32_t option=0;option<8;++option){auto graph=Graph();
        switch(option){
        case 0:graph.failure=1;break;
        case 1:graph.nodes.clear();break;
        case 2:graph=Graph(221);break;
        case 3:graph.nodes[1].handle=graph.nodes[0].handle;break;
        case 4:graph.nodes[1].order=0;break;
        case 5:graph.nodes[1].bytes[0x1d]=0;break;
        case 6:graph.nodes[1].bytes[0x1d]=220;break;
        case 7:graph.nodes[0].handle|=0x80000000;break;
        }
        memory=Fixture();Rejected(memory,resource_uses::Failure::SourceGraph,graph);
    }
}
void NativeBounds(){
    for(const auto offset:{resource_uses::UseCount,resource_uses::AuxCount,resource_uses::BoundaryCount}){
        auto memory=Fixture();memory.Put(Pool+offset,uint32_t(UINT_MAX));Rejected(memory,resource_uses::Failure::Limit);
    }
    for(uint8_t mode:{uint8_t(0),uint8_t(2),uint8_t(3),uint8_t(255)}){
        auto memory=Fixture();memory.values.at(Pool)[2]=mode;Rejected(memory,resource_uses::Failure::Header);
    }
    {auto memory=Fixture();memory.Ids({Id(2,0,0x123)});Rejected(memory,resource_uses::Failure::Slot);}
    {auto memory=Fixture();memory.Ids({Id(65535,0,0x123)});Rejected(memory,resource_uses::Failure::Slot);}
    {auto memory=Fixture();memory.Ids({Id(0,2,0x123)});Rejected(memory,resource_uses::Failure::Index);}
    {auto memory=Fixture();memory.Put(Pool+resource_uses::DeclarationCount,uint32_t(resource_uses::MaxDeclarations+1));Rejected(memory,resource_uses::Failure::Limit);}
    {auto memory=Fixture();memory.Ids({Id(0,0,0x124)});Rejected(memory,resource_uses::Failure::Declaration);}
    {auto memory=Fixture();memory.Ids({Id(0,0,0x123),Id(0,0,0x124)});Rejected(memory,resource_uses::Failure::Declaration);}
    {auto memory=Fixture();memory.values.at(Pool+resource_uses::Declarations)[1]=1;Rejected(memory,resource_uses::Failure::Declaration);}
    {auto memory=Fixture();memory.values.erase(Pool+resource_uses::Declarations);Rejected(memory,resource_uses::Failure::Declaration);}
    {auto memory=Fixture();memory.values.at(Pool+resource_uses::UseIds).pop_back();Rejected(memory,resource_uses::Failure::Ids);}
    {auto memory=Fixture();auto graph=Graph(1);graph.nodes[0].bytes[0x1d]=219;
        memory.Put(Pool+219*resource_uses::SlotStride+resource_uses::DeclarationCount,uint32_t(500));
        std::array<uint8_t,48> row{};row[0]=1;row[1]=219;Set(row,4,uint32_t(123));
        memory.Put(Pool+219*resource_uses::SlotStride+resource_uses::Declarations+499*48,row);
        memory.Ids({Id(219,499,123)});resource_uses::Snapshot out;Require(Capture(memory,out,graph));}
}
void AuxAndBoundaries(){
    auto memory=Fixture();memory.Put(Pool+resource_uses::AuxCount,uint32_t(2));
    memory.Put(Pool+resource_uses::AuxKeys,std::array<uint32_t,2>{0x123,0x456});
    memory.Put(Pool+resource_uses::BoundaryCount,uint32_t(1));memory.Put(Pool+resource_uses::BoundaryIds,uint64_t(1));
    auto& row=memory.values.at(Pool+resource_uses::Declarations+48);row[0]=11;uint32_t subtype=1;std::memcpy(row.data()+8,&subtype,4);
    resource_uses::Snapshot out;Require(Capture(memory,out) && out.auxiliaryKeys.size()==2 && out.boundaryIds==std::vector<uint64_t>{1});
    memory.calls.clear();memory.Put(Pool+resource_uses::BoundaryIds,Id(0,1,1));Rejected(memory,resource_uses::Failure::Ids);
    memory=Fixture();memory.Put(Pool+resource_uses::BoundaryCount,uint32_t(1));memory.Put(Pool+resource_uses::BoundaryIds,uint64_t(5));
    Rejected(memory,resource_uses::Failure::Ids);
    memory=Fixture();memory.Put(Pool+resource_uses::BoundaryCount,uint32_t(1));memory.Put(Pool+resource_uses::BoundaryIds,uint64_t(1));
    Rejected(memory,resource_uses::Failure::Declaration);
}
void ChangedReads(){
    for(const auto address:{Pool,Pool+resource_uses::UseCount,Pool+resource_uses::AuxCount,Pool+resource_uses::BoundaryCount,
        Pool+resource_uses::UseIds,Pool+resource_uses::Declarations,Pool+resource_uses::DeclarationCount}){
        auto memory=Fixture();memory.before=[address](auto& state,uint64_t at,size_t call){
            if(at==address && call==2)state.values.at(at)[at==Pool?2:0]^=1;
        };
        Rejected(memory,resource_uses::Failure::Changed);
    }
    auto memory=Fixture();memory.Put(Pool+resource_uses::AuxCount,uint32_t(1));
    memory.Put(Pool+resource_uses::AuxKeys,uint32_t(0x123));
    memory.before=[](auto& state,uint64_t at,size_t call){
        if(at==Pool+resource_uses::AuxKeys && call==2)state.values.at(at)[0]^=1;
    };
    Rejected(memory,resource_uses::Failure::Changed);
}
}
int main()try{
    Valid();ArgumentsAndGraph();NativeBounds();AuxAndBoundaries();ChangedReads();
    std::puts("PASS bounded native resource-use snapshots preserve opaque lifetimes and reject changed inputs");return 0;
}catch(const std::exception& error){std::fprintf(stderr,"%s\n",error.what());return 1;}
