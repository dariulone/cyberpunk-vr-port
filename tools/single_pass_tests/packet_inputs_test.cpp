#include "Render/NativePacketInputs.hpp"
#include <unordered_map>
#include <vector>
#include <iostream>
#include <cstdlib>
using namespace cvr::stereo::packets;
void Require(bool value,const char* text){if(!value){std::cerr<<text<<'\n';std::exit(1);}}
struct Memory {
    std::unordered_map<uint64_t,std::vector<uint8_t>> regions;
    unsigned calls{};
    static bool Read(void* opaque,uint64_t address,void* output,size_t bytes){
        auto& self=*static_cast<Memory*>(opaque);++self.calls;
        const auto at=self.regions.find(address);if(at==self.regions.end() || bytes!=at->second.size())return false;
        std::memcpy(output,at->second.data(),bytes);return true;
    }
};
int main(){
    constexpr uint64_t pool=0x10000000;
    Packet packet{(uint64_t(3)<<17)|(uint64_t(5)<<32)|(uint64_t(7)<<45)|0x1234,
                  11|(uint64_t(1)<<18)|(uint64_t(13)<<33)|(uint64_t(1)<<51)};
    Memory m;
    const uint64_t addresses[]{pool+0x50+3*64,pool+0x2541D0+5*6,pool+0x240110+7*32,pool+0x270248+11*24,pool+0x570280+13*48};
    const unsigned sizes[]{64,6,32,24,48};
    for(unsigned i=0;i<5;++i)m.regions[addresses[i]]=std::vector<uint8_t>(sizes[i],uint8_t(i+1));
    m.regions[addresses[0]][0]=0;
    auto& state=m.regions[addresses[2]];std::memset(state.data()+0x10,0,4);
    auto resolved=ResolveInputs(packet,pool,Memory::Read,&m);
    Require(resolved.valid==63 && resolved.instances==1 && resolved.commandProgramField==0,"complete direct packet");
    Require(m.calls==5 && resolved.shaders[5]==2 && resolved.shaders[6]==0 && resolved.shaders[7]==0,"exact native strides, zero padding");
    Require(resolved.draw[63]==1 && resolved.object[23]==4 && resolved.transform[47]==5,"complete owned entries");
    m.regions[addresses[0]][1]=9;Require(resolved.draw[1]==1,"independent snapshot");
    state[0x10]=7;resolved=ResolveInputs(packet,pool,Memory::Read,&m);Require(resolved.commandProgramField==7,"nonempty programs stay explicit");
    auto many=packet;many.second+=uint64_t(1)<<18;resolved=ResolveInputs(many,pool,Memory::Read,&m);
    Require(resolved.instances==2 && resolved.valid==47,"multi-instance matrices not falsely represented by one transform");
    const uint64_t bone=0x789000;std::memcpy(m.regions[addresses[3]].data()+8,&bone,8);
    m.regions[bone+0x14]=std::vector<uint8_t>(16,0xab);auto skin=packet;skin.second|=uint64_t(1)<<50;
    resolved=ResolveInputs(skin,pool,Memory::Read,&m);Require(resolved.valid==63 && resolved.skin[15]==0xab,"native skin metadata captured");
    m.regions.erase(bone+0x14);resolved=ResolveInputs(skin,pool,Memory::Read,&m);Require(resolved.valid==31,"unreadable skin does not pass");
    const uint64_t noBone=0;std::memcpy(m.regions[addresses[3]].data()+8,&noBone,8);
    resolved=ResolveInputs(skin,pool,Memory::Read,&m);Require(resolved.valid==63 && resolved.skin[12]==1,"native null-skin fallback is 0,0,0,1");
    std::memset(state.data()+0x10,0,4);
    std::vector<uint8_t> blob;
    auto details=ResolveDetails(packet,pool,{},Memory::Read,&m,blob);
    Require(details.valid==127 && details.instanceBytes==48 && details.instanceStride==48,"complete CPU instance");
    m.regions[addresses[4]]=std::vector<uint8_t>(96,0x36);blob.clear();
    details=ResolveDetails(many,pool,{},Memory::Read,&m,blob);Require(details.valid==127 && details.instanceBytes==96 && blob[95]==0x36,"all CPU instances copied");
    auto skinMany=many;skinMany.second|=uint64_t(1)<<50;blob.clear();
    details=ResolveDetails(skinMany,pool,{},Memory::Read,&m,blob);
    Require(details.valid==127 && details.instanceBytes==128 && details.instanceStride==64 && blob[60]==1 && blob[124]==1,"skin metadata appended to every instance");
    auto gpuPacket=many;gpuPacket.first|=uint64_t(1)<<59;
    constexpr uint64_t cpu=0x800000,gpu=0x900000;MappedInstances uploaded{cpu,gpu,1024,0x123456};
    m.regions[cpu+13*48]=std::vector<uint8_t>(96,0xa7);blob.clear();
    details=ResolveDetails(gpuPacket,pool,uploaded,Memory::Read,&m,blob);
    Require(details.valid==127 && details.source==2 && details.instanceGpuAddress==gpu+13*48 && details.instanceResource==0x123456 && blob[0]==0xa7,"preuploaded packets use actual instance buffer");
    uploaded.bytes=13*48+95;blob.clear();details=ResolveDetails(gpuPacket,pool,uploaded,Memory::Read,&m,blob);Require(!(details.valid&16),"preuploaded range bound");
    m.regions[addresses[0]][0]=2;blob.clear();details=ResolveDetails(many,pool,{},Memory::Read,&m,blob);Require(details.source==0 && !(details.valid&16),"view-facing geometry excluded");m.regions[addresses[0]][0]=0;
    const uint64_t program=0x123000;std::memcpy(state.data()+0x08,&program,8);uint32_t programBytes=17;std::memcpy(state.data()+0x10,&programBytes,4);
    m.regions[program]={1,0,0,0,0x80,0,0,0,7,0,0,0,1,0,0,0,2};blob.clear();
    details=ResolveDetails(many,pool,{},Memory::Read,&m,blob);Require(details.valid==127 && details.programBytes==17 && blob[details.programOffset+16]==2,"native command program copied");
    auto compareGeometry=[&](Packet p,MappedInstances mapped){
        std::vector<uint8_t> allBytes,geometryBytes;const auto before=m.calls;
        const auto all=ResolveDetails(p,pool,mapped,Memory::Read,&m,allBytes);const auto fullCalls=m.calls-before;
        const auto middle=m.calls;const auto geometry=ResolveDetails(p,pool,mapped,Memory::Read,&m,geometryBytes,true);
        Require(EqualGeometry(all,allBytes,geometry,geometryBytes),"geometry projection changed membership input");
        Require(m.calls-middle<fullCalls && !geometry.programBytes && !geometry.commandProgramField && !(geometry.valid&6),"geometry path still resolves unused shader/program data");
        if(geometry.valid&16){auto bad=geometryBytes;bad[geometry.instanceOffset]^=1;
            Require(!EqualGeometry(all,allBytes,geometry,bad),"changed instance accepted by projection comparison");
            auto range=geometry;range.instanceOffset=UINT32_MAX;Require(!EqualGeometry(all,allBytes,range,geometryBytes),"invalid projection range accepted");}
    };
    compareGeometry(many,{});compareGeometry(skinMany,{});uploaded.bytes=1024;compareGeometry(gpuPacket,uploaded);
    m.regions[addresses[4]]=std::vector<uint8_t>(48,0x36);compareGeometry(packet,{});compareGeometry(skin,{});
    m.regions.erase(addresses[4]);compareGeometry(packet,{});
    m.regions[addresses[0]][0]=2;compareGeometry(packet,{});m.regions[addresses[0]][0]=0;
    m.regions[addresses[4]]=std::vector<uint8_t>(96,0x36);
    m.regions[program][12]=2;blob.clear();details=ResolveDetails(many,pool,{},Memory::Read,&m,blob);Require(!(details.valid&64),"truncated command operands rejected");
    m.regions[addresses[4]]=std::vector<uint8_t>(48,5);
    m.regions.erase(addresses[1]);resolved=ResolveInputs(packet,pool,Memory::Read,&m);Require(resolved.valid==61,"missing shader data rejected");
    m.calls=0;resolved=ResolveInputs(packet,UINT64_MAX-16,Memory::Read,&m);Require(m.calls==0 && resolved.valid==32,"overflowed source addresses rejected");
    Require(ResolveInputs(packet,0,Memory::Read,&m).valid==0 && ResolveInputs(packet,pool,nullptr,&m).valid==0,"missing pool/reader");
    std::cout<<"Native input resolution: tables, programs, all instances, CPU/upload sources, skin fallback, view-facing exclusions and malformed data passed\n";
}
